use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use base64::{Engine, engine::general_purpose::STANDARD};
use lopdf::Object;

use super::{Extracted, le16, le32};

const INFO_MAX: u64 = 1024 * 1024;
const PDF_MAX: u64 = 128 * 1024 * 1024;

pub fn office(path: &Path) -> Result<Extracted, String> {
    let mut compound =
        cfb::open(path).map_err(|error| format!("无法读取 Office 容器（{error}）。"))?;
    if !compound.is_stream("/EncryptedPackage") || !compound.is_stream("/EncryptionInfo") {
        return Err("找不到 Office 打开密码的加密信息。此入口支持现代 Office 的文件加密。".into());
    }
    let mut info = Vec::new();
    compound
        .open_stream("/EncryptionInfo")
        .and_then(|stream| stream.take(INFO_MAX + 1).read_to_end(&mut info))
        .map_err(|error| format!("无法读取 Office 加密信息（{error}）。"))?;
    if info.len() as u64 > INFO_MAX {
        return Err("Office 加密信息异常过大。".into());
    }
    let result = match (le16(&info, 0), le16(&info, 2)) {
        (Some(4), Some(4)) => agile_office(&info),
        (Some(3 | 4), Some(2)) => standard_office(&info),
        _ => None,
    };
    result.ok_or_else(|| "此 Office 文件的加密类型或校验参数不受支持。".into())
}

fn agile_office(info: &[u8]) -> Option<Extracted> {
    let xml = std::str::from_utf8(info.get(8..)?).ok()?;
    let document = roxmltree::Document::parse(xml).ok()?;
    let key = document.descendants().find(|node| {
        node.has_tag_name((
            "http://schemas.microsoft.com/office/2006/keyEncryptor/password",
            "encryptedKey",
        ))
    })?;
    if key.attribute("cipherAlgorithm")? != "AES"
        || key.attribute("cipherChaining")? != "ChainingModeCBC"
    {
        return None;
    }
    let bits = key.attribute("keyBits")?;
    let (mode, year, format) = match (key.attribute("hashAlgorithm")?, bits) {
        ("SHA1", "128") => (9500, 2010, "Office Agile SHA-1"),
        ("SHA512", "256") => (9600, 2013, "Office Agile SHA-512"),
        _ => return None,
    };
    let iterations: u32 = key.attribute("spinCount")?.parse().ok()?;
    if !(1..=10_000_000).contains(&iterations) {
        return None;
    }
    let salt = STANDARD.decode(key.attribute("saltValue")?).ok()?;
    let verifier = STANDARD
        .decode(key.attribute("encryptedVerifierHashInput")?)
        .ok()?;
    let verifier_hash = STANDARD
        .decode(key.attribute("encryptedVerifierHashValue")?)
        .ok()?;
    if salt.len() != 16 || verifier.len() != 16 {
        return None;
    }
    let hash = format!(
        "$office$*{year}*{iterations}*{bits}*16*{}*{}*{}",
        hex::encode(salt),
        hex::encode(verifier),
        hex::encode(verifier_hash.get(..32)?)
    );
    Some(Extracted { mode, format, hash })
}

fn standard_office(info: &[u8]) -> Option<Extracted> {
    let header_size = le32(info, 8)? as usize;
    let header = info.get(12..12_usize.checked_add(header_size)?)?;
    if header.len() < 32
        || le32(header, 8)? != 0x660e
        || le32(header, 12)? != 0x8004
        || le32(header, 16)? != 128
    {
        return None;
    }
    let verifier = info.get(12 + header_size..)?;
    if le32(verifier, 0)? != 16 || le32(verifier, 36)? != 20 {
        return None;
    }
    let hash = format!(
        "$office$*2007*20*128*16*{}*{}*{}",
        hex::encode(verifier.get(4..20)?),
        hex::encode(verifier.get(20..36)?),
        hex::encode(verifier.get(40..60)?)
    );
    Some(Extracted {
        mode: 9400,
        format: "Office Standard AES",
        hash,
    })
}

pub fn pdf(mut file: File) -> Result<Extracted, String> {
    let length = file
        .metadata()
        .map_err(|error| format!("无法读取 PDF 文件大小（{error}）。"))?
        .len();
    if length > PDF_MAX {
        return Err("此 PDF 超过 128 MiB，无法在界面内提取校验数据。".into());
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|error| format!("无法读取 PDF（{error}）。"))?;
    let document =
        lopdf::Document::load_from(file).map_err(|error| format!("无法解析 PDF（{error}）。"))?;
    let dictionary = document
        .get_encrypted()
        .map_err(|_| "此 PDF 可以直接打开，没有检测到打开密码加密。")?;
    let parsed = (|| {
        if dictionary.get(b"Filter").ok()?.as_name().ok()? != b"Standard" {
            return None;
        }
        let number = |key: &[u8]| dictionary.get(key).ok()?.as_i64().ok();
        let version = number(b"V").unwrap_or(1);
        let revision = number(b"R")?;
        let bits = match revision {
            2 => 40,
            3 => number(b"Length").unwrap_or(40),
            4 => number(b"Length").unwrap_or(128),
            5 | 6 => 256,
            _ => return None,
        };
        let (mode, format) = match (version, revision) {
            (1, 2) => (10400, "PDF RC4 R2"),
            (2, 3) | (4, 4) => (10500, "PDF Standard R3 / R4"),
            (5, 5) => (10600, "PDF AES-256 R5"),
            (5, 6) => (10700, "PDF AES-256 R6"),
            _ => return None,
        };
        let permissions = pdf_permissions(number(b"P")?)?;
        let metadata = if matches!(
            dictionary.get(b"EncryptMetadata"),
            Ok(Object::Boolean(false))
        ) {
            0
        } else {
            1
        };
        let id = document
            .trailer
            .get(b"ID")
            .ok()?
            .as_array()
            .ok()?
            .first()?
            .as_str()
            .ok()?;
        let bytes = |key: &[u8]| dictionary.get(key).ok()?.as_str().ok();
        let user = bytes(b"U")?;
        let owner = bytes(b"O")?;
        let expected = if revision >= 5 { 48 } else { 32 };
        if !matches!(id.len(), 0 | 16 | 32) || user.len() != expected || owner.len() != expected {
            return None;
        }
        let mut hash = format!(
            "$pdf${version}*{revision}*{bits}*{permissions}*{metadata}*{}*{}*{}*{}*{}*{}",
            id.len(),
            hex::encode(id),
            user.len(),
            hex::encode(user),
            owner.len(),
            hex::encode(owner)
        );
        if revision >= 5 {
            let user_key = bytes(b"UE")?;
            let owner_key = bytes(b"OE")?;
            if user_key.len() != 32 || owner_key.len() != 32 {
                return None;
            }
            hash.push_str(&format!(
                "*32*{}*32*{}",
                hex::encode(user_key),
                hex::encode(owner_key)
            ));
        }
        Some(Extracted { mode, format, hash })
    })();
    parsed.ok_or_else(|| "此 PDF 的加密类型或校验参数不受支持。".into())
}

fn pdf_permissions(value: i64) -> Option<i32> {
    // Some PDF writers serialize these bits as unsigned. Hashcat uses signed C long on Windows.
    (i64::from(i32::MIN)..=i64::from(u32::MAX))
        .contains(&value)
        .then_some(value as i32)
}

#[cfg(test)]
mod tests {
    use super::pdf_permissions;

    #[test]
    fn converts_both_pdf_permission_encodings_to_the_same_bits() {
        assert_eq!(pdf_permissions(-4), Some(-4));
        assert_eq!(pdf_permissions(4_294_967_292), Some(-4));
        assert_eq!(pdf_permissions(4_294_967_296), None);
    }
}
