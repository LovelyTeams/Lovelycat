use sha2::{Digest, Sha256};
use std::{
    env,
    fs::{self, File},
    io::{self, BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Resource {
    Hashcat,
    RockYou,
}

impl Resource {
    pub fn installed(self, data: &Path) -> PathBuf {
        match self {
            Self::Hashcat => data.join("resources/hashcat-8.0.0/hashcat.exe"),
            Self::RockYou => data.join("resources/rockyou/rockyou.txt"),
        }
    }

    pub fn install(self, data: &Path) -> Result<PathBuf, String> {
        let installed = self.installed(data);
        if installed.is_file() {
            return Ok(installed);
        }
        let base = data.join("resources");
        fs::create_dir_all(&base).map_err(|e| e.to_string())?;
        let stage = tempfile::tempdir_in(&base).map_err(|e| e.to_string())?;
        let archive = stage.path().join("package");
        let (url, digest) = match self {
            Self::Hashcat => (
                "https://hashcat.net/files/hashcat-8.0.0.7z",
                "9fa86e1bfd5dd898d3c83aa38a67b08db1ba5d2f5252dffa2af44a8c62585f49",
            ),
            Self::RockYou => (
                "https://gitlab.com/kalilinux/packages/wordlists/-/raw/249f397cdbb3d7293c1f128131ea16345ce676e7/rockyou.txt.gz",
                "ded2d962815e1256df8f3a0d25173c4b21b6eee636117c36999246725a6d8f9f",
            ),
        };
        download(url, digest, &archive)?;
        let extracted = match self {
            Self::Hashcat => {
                sevenz_rust2::decompress_file(&archive, stage.path())
                    .map_err(|e| format!("解压 Hashcat 失败（{e}）"))?;
                let dir = stage.path().join("hashcat-8.0.0");
                if !dir.join("hashcat.exe").is_file() || !dir.join("modules").is_dir() {
                    return Err("发行包缺少 Hashcat 程序或模块。".into());
                }
                dir
            }
            Self::RockYou => {
                let dir = stage.path().join("rockyou");
                fs::create_dir(&dir).map_err(|e| e.to_string())?;
                let mut reader = flate2::read::GzDecoder::new(BufReader::new(
                    File::open(&archive).map_err(|e| e.to_string())?,
                ));
                let mut output =
                    File::create(dir.join("rockyou.txt")).map_err(|e| e.to_string())?;
                io::copy(&mut reader, &mut output).map_err(|e| format!("解压字典失败（{e}）"))?;
                dir
            }
        };
        let destination = installed.parent().expect("resource has a directory");
        if let Err(e) = fs::rename(&extracted, destination)
            && !installed.is_file()
        {
            return Err(format!("无法安装到 {}（{e}）", destination.display()));
        }
        Ok(installed)
    }
}

pub(crate) fn download(url: &str, digest: &str, archive: &Path) -> Result<(), String> {
    let curl = env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("System32/curl.exe");
    let mut cmd = Command::new(curl);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    }
    let result = cmd
        .args([
            "--disable",
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--connect-timeout",
            "15",
            "--max-time",
            "600",
            "--retry",
            "2",
            "--output",
        ])
        .arg(archive)
        .arg(url)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("无法启动系统下载程序（{e}）"))?;
    if !result.status.success() {
        return Err(format!(
            "下载失败（{}）",
            String::from_utf8_lossy(&result.stderr).trim()
        ));
    }
    verify(archive, digest)
}

fn verify(path: &Path, expected: &str) -> Result<(), String> {
    let mut reader = File::open(path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let len = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if len == 0 {
            break;
        }
        hash.update(&buffer[..len]);
    }
    if hex::encode(hash.finalize()) != expected {
        return Err("下载文件的 SHA-256 校验失败，请重试。".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn modified_archives_fail_integrity_checks() {
        let file = tempfile::NamedTempFile::new().unwrap();
        fs::write(file.path(), b"abc").unwrap();
        let expected = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert!(verify(file.path(), expected).is_ok());
        fs::write(file.path(), b"ABC").unwrap();
        assert!(verify(file.path(), expected).is_err());
    }
    #[test]
    #[ignore = "downloads and verifies the two pinned public release artifacts"]
    fn installs_both_resources_in_an_isolated_directory() {
        let temp = tempfile::tempdir().unwrap();
        for resource in [Resource::Hashcat, Resource::RockYou] {
            let installed = resource.install(temp.path()).unwrap();
            assert!(installed.is_file());
            assert_eq!(resource.install(temp.path()).unwrap(), installed);
            if resource == Resource::Hashcat {
                let probe = crate::hashcat::probe(&installed).unwrap();
                assert_eq!(probe.version, "v8.0.0");
                assert!(!probe.modes.is_empty());
            } else {
                assert!(installed.metadata().unwrap().len() > 100_000_000);
            }
        }
    }
}
