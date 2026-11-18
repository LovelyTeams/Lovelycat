mod archives;
mod documents;

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

pub struct Extracted {
    pub mode: u32,
    pub format: &'static str,
    pub hash: String,
}

pub fn extract(path: &Path) -> Result<Extracted, String> {
    let mut file = File::open(path).map_err(|error| format!("无法打开文件（{error}）。"))?;
    let mut signature = [0; 8];
    file.read_exact(&mut signature)
        .map_err(|error| format!("无法读取文件头（{error}）。"))?;
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let office = matches!(
        extension.as_str(),
        "docx" | "xlsx" | "pptx" | "docm" | "xlsm" | "pptm" | "xlsb" | "dotx" | "xltx" | "potx"
    );
    if signature == [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1] {
        return documents::office(path);
    }
    if signature.starts_with(b"PK") && office {
        return Err("此 Office 文件可以直接打开，没有检测到打开密码加密。".into());
    }
    if signature.starts_with(b"PK\x03\x04")
        || signature.starts_with(b"PK\x05\x06")
        || extension == "zip"
    {
        return archives::zip(path, file);
    }
    if signature == *b"Rar!\x1a\x07\x01\x00" {
        return archives::rar5(file);
    }
    if signature.starts_with(b"Rar!\x1a\x07\x00") {
        return archives::rar3(file);
    }
    if signature.starts_with(b"7z\xbc\xaf\x27\x1c") {
        return archives::seven_zip(file);
    }
    if signature.starts_with(b"%PDF-") || extension == "pdf" {
        return documents::pdf(file);
    }
    Err("无法识别此文件。支持 ZIP、RAR、7z、PDF 和使用打开密码加密的 Office 文档。".into())
}

fn read_at(file: &mut File, offset: u64, size: usize) -> io::Result<Vec<u8>> {
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = vec![0; size];
    file.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn take<'a>(data: &mut &'a [u8], count: usize) -> Option<&'a [u8]> {
    let (head, tail) = data.split_at_checked(count)?;
    *data = tail;
    Some(head)
}

fn le16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..)?.first_chunk().copied()?,
    ))
}

fn le32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..)?.first_chunk().copied()?,
    ))
}

fn le64(bytes: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        bytes.get(offset..)?.first_chunk().copied()?,
    ))
}
