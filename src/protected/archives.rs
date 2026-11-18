use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use super::{Extracted, le16, le32, le64, read_at, take};

const MAX_PKZIP: u64 = 320 * 1024;
const MAX_ARCHIVE_BLOCK: u64 = 8 * 1024 * 1024 - 1;
const MAX_SEVEN_ZIP_UNPACKED: u64 = 64 * 1024 * 1024;
const MAX_RAR_HEADER: u64 = 2 * 1024 * 1024;
const AES_7Z: &[u8] = &[0x06, 0xf1, 0x07, 0x01];

struct ZipMember {
    name: String,
    offset: u64,
    data_offset: u64,
    packed: u64,
    unpacked: u64,
    crc: u32,
    method: u16,
    dos_time: u16,
    aes_strength: Option<u8>,
}

pub fn zip(path: &Path, mut file: File) -> Result<Extracted, String> {
    let archive = File::open(path).map_err(zip_error)?;
    let mut archive = zip::ZipArchive::new(BufReader::new(archive)).map_err(zip_error)?;
    let mut encrypted = false;
    let mut best: Option<ZipMember> = None;
    for index in 0..archive.len() {
        let entry = archive.by_index_raw(index).map_err(zip_error)?;
        if !entry.encrypted() || entry.is_dir() {
            continue;
        }
        encrypted = true;
        let packed = entry.compressed_size();
        if packed > MAX_ARCHIVE_BLOCK || best.as_ref().is_some_and(|best| best.packed <= packed) {
            continue;
        }
        let offset = entry.header_start();
        let header = read_at(&mut file, offset, 30).map_err(zip_error)?;
        let flags = le16(&header, 6).unwrap_or_default();
        let method = le16(&header, 8).unwrap_or_default();
        let aes_strength = match method {
            99 => aes_zip_strength(&mut file, offset, &header).map_err(zip_error)?,
            0 | 8 if flags & 0x40 == 0 && (12..=MAX_PKZIP).contains(&packed) => None,
            _ => continue,
        };
        if method == 99
            && !aes_strength.is_some_and(|strength| {
                zip_salt_len(strength).is_some_and(|salt| packed >= (salt as u64) + 12)
            })
        {
            continue;
        }
        best = Some(ZipMember {
            name: entry.name().into(),
            offset,
            packed,
            method,
            aes_strength,
            data_offset: entry.data_start(),
            unpacked: entry.size(),
            crc: entry.crc32(),
            dos_time: le16(&header, 10).unwrap_or_default(),
        });
    }
    let best = best.ok_or_else(|| {
        if encrypted {
            "ZIP 中没有可用的校验成员。支持常见 WinZip AES 或 ZipCrypto 成员，成员大小和压缩方式须符合 Hashcat 限制。"
        } else {
            "ZIP 中没有找到受密码保护的文件。"
        }.to_owned()
    })?;
    let bytes = read_at(&mut file, best.data_offset, best.packed as usize).map_err(zip_error)?;
    if let Some(strength) = best.aes_strength {
        let salt_len = zip_salt_len(strength).expect("validated AES strength");
        let salt = hex::encode(&bytes[..salt_len]);
        let check = hex::encode(&bytes[salt_len..salt_len + 2]);
        let auth = hex::encode(&bytes[bytes.len() - 10..]);
        let payload = &bytes[salt_len + 2..bytes.len() - 10];
        let hash = format!(
            "$zip2$*0*{strength}*0*{salt}*{check}*{:x}*{}*{auth}*$/zip2$",
            payload.len(),
            hex::encode(payload)
        );
        return Ok(Extracted {
            mode: 13600,
            format: "ZIP AES",
            hash,
        });
    }
    let mode = if best.method == 0 { 17210 } else { 17200 };
    let relative = best
        .data_offset
        .checked_sub(best.offset)
        .ok_or_else(|| format!("ZIP 成员 {} 的偏移量无效。", best.name))?;
    let hash = format!(
        "$pkzip2$1*1*2*0*{:x}*{:x}*{:08x}*{:x}*{:x}*{}*{:x}*{:04x}*{:04x}*{}*$/pkzip2$",
        best.packed,
        best.unpacked,
        best.crc,
        best.offset,
        relative,
        best.method,
        best.packed,
        best.crc >> 16,
        best.dos_time,
        hex::encode(bytes)
    );
    Ok(Extracted {
        mode,
        format: "ZIP ZipCrypto",
        hash,
    })
}

fn zip_error(error: impl std::fmt::Display) -> String {
    format!("无法读取 ZIP（{error}）。")
}

fn zip_salt_len(strength: u8) -> Option<usize> {
    match strength {
        1 => Some(8),
        2 => Some(12),
        3 => Some(16),
        _ => None,
    }
}

fn aes_zip_strength(file: &mut File, offset: u64, header: &[u8]) -> io::Result<Option<u8>> {
    let name_len = u64::from(le16(header, 26).unwrap_or_default());
    let size = usize::from(le16(header, 28).unwrap_or_default());
    let bytes = read_at(file, offset + 30 + name_len, size)?;
    let mut fields = bytes.as_slice();
    while let Some(record) = take(&mut fields, 4) {
        let kind = le16(record, 0).unwrap_or_default();
        let size = usize::from(le16(record, 2).unwrap_or_default());
        let Some(data) = take(&mut fields, size) else {
            break;
        };
        if kind == 0x9901 && data.get(2..4) == Some(b"AE") {
            return Ok(data.get(4).copied());
        }
    }
    Ok(None)
}

pub fn rar5(mut file: File) -> Result<Extracted, String> {
    scan_rar5(&mut file)
        .map_err(|error| format!("无法读取 RAR5（{error}）。"))?
        .ok_or_else(|| "RAR5 中没有找到 Hashcat 可用的密码校验记录。".into())
}

fn scan_rar5(file: &mut File) -> io::Result<Option<Extracted>> {
    let size = file.metadata()?.len();
    file.seek(SeekFrom::Start(8))?;
    while file.stream_position()? < size {
        let mut crc = [0; 4];
        file.read_exact(&mut crc)?;
        let size_pos = file.stream_position()?;
        let header_size = rar_number(file)?;
        let head_pos = file.stream_position()?;
        if header_size == 0
            || header_size > MAX_RAR_HEADER
            || header_size > size.saturating_sub(head_pos)
        {
            return Err(invalid_archive());
        }
        let bytes = read_at(file, size_pos, (head_pos - size_pos + header_size) as usize)?;
        if crc32fast::hash(&bytes) != u32::from_le_bytes(crc) {
            return Err(invalid_archive());
        }
        let mut header = &bytes[(head_pos - size_pos) as usize..];
        let kind = rar_number(&mut header)?;
        let flags = rar_number(&mut header)?;
        let extra = if flags & 1 != 0 {
            rar_number(&mut header)?
        } else {
            0
        };
        let payload = if flags & 2 != 0 {
            rar_number(&mut header)?
        } else {
            0
        };
        if extra > header.len() as u64 {
            return Err(invalid_archive());
        }
        let (body, mut extras) = header.split_at(header.len() - extra as usize);
        if kind == 4 {
            let mut iv = [0; 16];
            file.read_exact(&mut iv)?;
            return rar5_verifier(body, Some(&iv));
        }
        if kind == 2 {
            while !extras.is_empty() {
                let record_size = rar_number(&mut extras)?;
                if record_size == 0 || record_size > extras.len() as u64 {
                    return Err(invalid_archive());
                }
                let mut record =
                    take(&mut extras, record_size as usize).ok_or_else(invalid_archive)?;
                if rar_number(&mut record)? == 1 {
                    if let Some(candidate) = rar5_verifier(record, None)? {
                        return Ok(Some(candidate));
                    }
                }
            }
        }
        if kind == 5 {
            break;
        }
        let next = file
            .stream_position()?
            .checked_add(payload)
            .filter(|end| *end <= size)
            .ok_or_else(invalid_archive)?;
        file.seek(SeekFrom::Start(next))?;
    }
    Ok(None)
}

fn rar_number(from: &mut impl Read) -> io::Result<u64> {
    let mut result = 0_u64;
    let mut byte = [0; 1];
    for shift in (0..64).step_by(7) {
        from.read_exact(&mut byte)?;
        if shift == 63 && byte[0] > 1 {
            return Err(invalid_archive());
        }
        result |= u64::from(byte[0] & 0x7f) << shift;
        if byte[0] & 0x80 == 0 {
            return Ok(result);
        }
    }
    Err(invalid_archive())
}

fn rar5_verifier(mut fields: &[u8], header_iv: Option<&[u8; 16]>) -> io::Result<Option<Extracted>> {
    let version = rar_number(&mut fields)?;
    let flags = rar_number(&mut fields)?;
    let kdf = take(&mut fields, 1).ok_or_else(invalid_archive)?[0];
    if version != 0 || flags & 1 == 0 || !(1..=24).contains(&kdf) {
        return Ok(None);
    }
    let salt = take(&mut fields, 16).ok_or_else(invalid_archive)?;
    let iv = match header_iv {
        Some(iv) => &iv[..],
        None => take(&mut fields, 16).ok_or_else(invalid_archive)?,
    };
    let check = take(&mut fields, 12).ok_or_else(invalid_archive)?;
    let hash = format!(
        "$rar5$16${}${kdf:02}${}$8${}",
        hex::encode(salt),
        hex::encode(iv),
        hex::encode(&check[..8])
    );
    Ok(Some(Extracted {
        mode: 13000,
        format: "RAR5 AES",
        hash,
    }))
}

pub fn rar3(mut file: File) -> Result<Extracted, String> {
    let header = read_at(&mut file, 7, 7).map_err(|error| format!("无法读取 RAR3（{error}）。"))?;
    let flags = le16(&header, 3).unwrap_or_default();
    if header[2] != 0x73 || flags & 0x80 == 0 {
        return Err("此 RAR3 未加密文件名。直接导入支持 RAR3 文件头加密和 RAR5，请将其他 RAR3 文件先转换为 Hashcat 哈希。".into());
    }
    let tail = file
        .metadata()
        .ok()
        .and_then(|meta| meta.len().checked_sub(24))
        .filter(|offset| *offset >= 20)
        .ok_or_else(|| "RAR3 文件不完整。".to_owned())?;
    let bytes = read_at(&mut file, tail, 24)
        .map_err(|error| format!("无法读取 RAR3 校验数据（{error}）。"))?;
    let hash = format!(
        "$RAR3$*0*{}*{}",
        hex::encode(&bytes[..8]),
        hex::encode(&bytes[8..])
    );
    Ok(Extracted {
        mode: 12500,
        format: "RAR3 文件头加密",
        hash,
    })
}

fn invalid_archive() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "文件头无效或不完整")
}

struct SevenZipBlock {
    offset: u64,
    packed: u64,
    unpacked: u64,
    crc: u32,
    crc_len: u64,
    encryption: Vec<u8>,
    compression: u8,
    properties: Vec<u8>,
}

pub fn seven_zip(mut file: File) -> Result<Extracted, String> {
    let next =
        read_7z_header(&mut file).map_err(|error| format!("无法读取 7z 文件头（{error}）。"))?;
    if let Some(block) = encrypted_7z_header(&next) {
        return build_7z_hash(&mut file, block);
    }
    let archive = sevenz_rust2::Archive::read_with_memory_limit(
        &mut file,
        &sevenz_rust2::Password::empty(),
        64 * 1024,
    )
    .map_err(|error| format!("无法读取 7z 的加密信息（{error}）。"))?;
    let block = archive
        .blocks
        .iter()
        .enumerate()
        .filter_map(|(index, block)| {
            let mut coders = block.ordered_coder_iter();
            let (_, aes) = coders.next()?;
            if aes.encoder_method_id() != AES_7Z {
                return None;
            }
            let (compression, properties) = match coders.next() {
                None => (0, Vec::new()),
                Some((_, coder)) => {
                    let format = match coder.encoder_method_id() {
                        [0] => 0,
                        [0x03, 0x01, 0x01] => 1,
                        [0x21] => 2,
                        _ => return None,
                    };
                    (format, coder.properties().to_vec())
                }
            };
            if coders.next().is_some() {
                return None;
            }
            let stream_index = *archive
                .stream_map
                .block_first_pack_stream_index()
                .get(index)?;
            let packed = *archive.pack_sizes().get(stream_index)?;
            let unpacked = block.get_unpack_size_for_coder(aes);
            let (crc, crc_len) = if block.has_crc {
                (block.crc as u32, block.get_unpack_size())
            } else {
                let file = archive
                    .files
                    .get(*archive.stream_map.block_first_file_index.get(index)?)?;
                if !file.has_crc || (compression == 0 && file.size != unpacked) {
                    return None;
                }
                (file.crc as u32, file.size)
            };
            if packed > MAX_ARCHIVE_BLOCK || crc_len > MAX_SEVEN_ZIP_UNPACKED {
                return None;
            }
            let offset = 32_u64
                .checked_add(archive.pack_pos())?
                .checked_add(*archive.stream_map.pack_stream_offsets().get(stream_index)?)?;
            Some(SevenZipBlock {
                offset,
                packed,
                unpacked,
                crc,
                crc_len,
                encryption: aes.properties().to_vec(),
                compression,
                properties,
            })
        })
        .min_by_key(|block| block.packed)
        .ok_or_else(|| {
            "7z 中没有可用的加密校验块。支持 AES 文件头及常见 LZMA/LZMA2 或存储模式的加密成员。"
                .to_owned()
        })?;
    build_7z_hash(&mut file, block)
}

fn read_7z_header(file: &mut File) -> io::Result<Vec<u8>> {
    let begin = read_at(file, 0, 32)?;
    if begin[6] != 0 || le32(&begin, 8) != Some(crc32fast::hash(&begin[12..])) {
        return Err(invalid_archive());
    }
    let offset = le64(&begin, 12)
        .and_then(|offset| offset.checked_add(32))
        .ok_or_else(invalid_archive)?;
    let length = le64(&begin, 20)
        .filter(|length| *length <= MAX_ARCHIVE_BLOCK)
        .ok_or_else(invalid_archive)?;
    let next = read_at(file, offset, length as usize)?;
    if le32(&begin, 28) != Some(crc32fast::hash(&next)) {
        return Err(invalid_archive());
    }
    Ok(next)
}

fn encrypted_7z_header(mut data: &[u8]) -> Option<SevenZipBlock> {
    if take(&mut data, 2)? != [0x17, 0x06] {
        return None;
    }
    let offset = seven_number(&mut data)?.checked_add(32)?;
    if seven_number(&mut data)? != 1 || take(&mut data, 1)? != [0x09] {
        return None;
    }
    let packed = seven_number(&mut data)?;
    if data.first() == Some(&0x0a) {
        take(&mut data, 1)?;
        seven_crc(&mut data)?;
    }
    if take(&mut data, 3)? != [0x00, 0x07, 0x0b]
        || seven_number(&mut data)? != 1
        || take(&mut data, 1)? != [0]
    {
        return None;
    }
    if seven_number(&mut data)? != 1
        || take(&mut data, 1)? != [0x24]
        || take(&mut data, 4)? != AES_7Z
    {
        return None;
    }
    let prop_len = usize::try_from(seven_number(&mut data)?).ok()?;
    let encryption = take(&mut data, prop_len)?.to_vec();
    if take(&mut data, 1)? != [0x0c] {
        return None;
    }
    let unpacked = seven_number(&mut data)?;
    if take(&mut data, 1)? != [0x0a] {
        return None;
    }
    let crc = seven_crc(&mut data)??;
    Some(SevenZipBlock {
        offset,
        packed,
        unpacked,
        crc,
        crc_len: unpacked,
        encryption,
        compression: 0,
        properties: Vec::new(),
    })
}

fn seven_number(data: &mut &[u8]) -> Option<u64> {
    let first = take(data, 1)?[0];
    let extra = first.leading_ones() as usize;
    let mut low = [0_u8; 8];
    low[..extra].copy_from_slice(take(data, extra)?);
    if extra < 8 {
        low[extra] = first & (0x7f >> extra);
    }
    Some(u64::from_le_bytes(low))
}

fn seven_crc(data: &mut &[u8]) -> Option<Option<u32>> {
    let all = take(data, 1)?[0];
    let present = all != 0 || take(data, 1)?[0] & 0x80 != 0;
    if present {
        Some(Some(le32(take(data, 4)?, 0)?))
    } else {
        Some(None)
    }
}

fn build_7z_hash(file: &mut File, block: SevenZipBlock) -> Result<Extracted, String> {
    let params = (|| {
        let (&first, properties) = block.encryption.split_first()?;
        let power = first & 0x3f;
        if power > 24
            || block.packed == 0
            || block.packed > MAX_ARCHIVE_BLOCK
            || block.packed % 16 != 0
            || block.unpacked > block.packed
            || block.packed - block.unpacked > 15
        {
            return None;
        }
        let (salt, iv) = if first & 0xc0 == 0 {
            (&[][..], &[][..])
        } else {
            let (&size, values) = properties.split_first()?;
            let salt_len = usize::from((first >> 7) + (size >> 4));
            let iv_len = usize::from(((first >> 6) & 1) + (size & 0x0f));
            if salt_len > 7 || iv_len > 16 {
                return None;
            }
            (
                values.get(..salt_len)?,
                values.get(salt_len..salt_len + iv_len)?,
            )
        };
        Some((power, salt, iv))
    })()
    .ok_or_else(|| "此 7z 加密块的大小或加密参数超出 Hashcat 支持范围。".to_owned())?;
    let (power, salt, iv) = params;
    let mut padded_iv = [0; 16];
    padded_iv[..iv.len()].copy_from_slice(iv);
    let data = read_at(file, block.offset, block.packed as usize)
        .map_err(|error| format!("无法读取 7z 校验块（{error}）。"))?;
    let mut hash = format!(
        "$7z${}${power}${}${}${}${}${}${}${}${}",
        block.compression,
        salt.len(),
        hex::encode(salt),
        iv.len(),
        hex::encode(padded_iv),
        block.crc,
        block.packed,
        block.unpacked,
        hex::encode(data)
    );
    if block.compression != 0 {
        hash.push_str(&format!(
            "${}${}",
            block.crc_len,
            hex::encode(block.properties)
        ));
    }
    Ok(Extracted {
        mode: 11600,
        format: "7z AES",
        hash,
    })
}
