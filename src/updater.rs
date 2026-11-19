use std::{
    env,
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use serde::Deserialize;
use sha2::{Digest, Sha256};

pub const REPO: &str = "LovelyTeams/Lovelycat";
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug)]
pub struct ReleaseAsset {
    pub name: String,
    pub download_url: String,
    pub size: u64,
}

#[derive(Clone, Debug)]
pub struct ReleaseInfo {
    pub tag: String,
    pub version: String,
    pub title: String,
    pub notes: String,
    pub html_url: String,
    pub asset: Option<ReleaseAsset>,
    pub checksums_url: Option<String>,
}

#[derive(Clone, Debug)]
pub enum CheckOutcome {
    UpToDate,
    NoRelease,
    Available(ReleaseInfo),
}

#[derive(Clone, Debug)]
pub struct PreparedUpdate {
    pub version: String,
    pub artifact_path: PathBuf,
    pub binary_path: Option<PathBuf>,
}

pub enum ApplyAction {
    QuitToRestart,
    RevealPath(PathBuf),
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    body: String,
    html_url: String,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

#[derive(Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

pub fn check_latest() -> Result<CheckOutcome, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let mut cmd = curl_command();
    let result = cmd
        .args([
            "--disable",
            "--silent",
            "--show-error",
            "--location",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--connect-timeout",
            "10",
            "--max-time",
            "20",
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            &format!("User-Agent: Lovelycat/{CURRENT_VERSION}"),
            "-w",
            "\n%{http_code}",
        ])
        .arg(&url)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("无法启动网络请求（{e}）"))?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr).trim().to_owned();
        return Err(if stderr.is_empty() {
            "连接 GitHub 检查更新失败，请检查网络连接。".into()
        } else {
            format!("连接 GitHub 失败（{stderr}）")
        });
    }

    let raw = String::from_utf8_lossy(&result.stdout);
    let (body, status_str) = raw.rsplit_once('\n').unwrap_or((raw.as_ref(), ""));
    let status: u16 = status_str.trim().parse().unwrap_or(0);
    if status == 404 {
        return Ok(CheckOutcome::NoRelease);
    }
    if status == 403 {
        return Err("GitHub API 请求频率受限，请稍后再试。".into());
    }
    if !(200..300).contains(&status) {
        return Err(format!("GitHub 返回异常状态码（HTTP {status}）。"));
    }

    let release: GithubRelease =
        serde_json::from_str(body).map_err(|e| format!("解析 GitHub 发布信息失败（{e}）"))?;
    let latest_version = normalize_version(&release.tag_name).to_owned();
    if !is_newer(&latest_version, CURRENT_VERSION) {
        return Ok(CheckOutcome::UpToDate);
    }

    let assets: Vec<ReleaseAsset> = release
        .assets
        .into_iter()
        .map(|a| ReleaseAsset {
            name: a.name,
            download_url: a.browser_download_url,
            size: a.size,
        })
        .collect();
    let checksums_url = assets
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case("SHA256SUMS.txt"))
        .map(|a| a.download_url.clone());
    let asset = select_asset(&assets, env::consts::OS, env::consts::ARCH).cloned();

    Ok(CheckOutcome::Available(ReleaseInfo {
        tag: release.tag_name,
        version: latest_version,
        title: if release.name.trim().is_empty() {
            format!("Lovelycat v{}", normalize_version(CURRENT_VERSION))
        } else {
            release.name.trim().to_owned()
        },
        notes: release.body.trim().to_owned(),
        html_url: release.html_url,
        asset,
        checksums_url,
    }))
}

pub fn download_update(release: &ReleaseInfo, data: &Path) -> Result<PreparedUpdate, String> {
    let asset = release
        .asset
        .as_ref()
        .ok_or_else(|| "当前系统暂无可直接下载的安装包，请前往 GitHub 发布页查看。".to_owned())?;
    let safe_dir_name: String = release
        .tag
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let dir = data.join("updates").join(safe_dir_name);
    fs::create_dir_all(&dir).map_err(|e| format!("无法创建更新目录（{e}）"))?;

    let expected_sha = if let Some(sum_url) = &release.checksums_url {
        fetch_text(sum_url)
            .ok()
            .and_then(|text| parse_sha256_for_asset(&text, &asset.name))
    } else {
        None
    };

    let artifact_path = dir.join(&asset.name);
    let tmp_path = dir.join(format!("{}.part", asset.name));
    download_file(&asset.download_url, &tmp_path)?;
    if let Some(expected) = expected_sha {
        let actual = sha256_file(&tmp_path)?;
        if !actual.eq_ignore_ascii_case(&expected) {
            let _ = fs::remove_file(&tmp_path);
            return Err("更新包 SHA-256 校验失败，已中止安装。".into());
        }
    }
    let _ = fs::remove_file(&artifact_path);
    fs::rename(&tmp_path, &artifact_path).map_err(|e| format!("保存更新包失败（{e}）"))?;

    let binary_path = extract_binary(&artifact_path, &dir)?;
    Ok(PreparedUpdate {
        version: release.version.clone(),
        artifact_path,
        binary_path,
    })
}

pub fn apply_and_restart(prepared: &PreparedUpdate) -> Result<ApplyAction, String> {
    #[cfg(windows)]
    {
        let Some(new_bin) = &prepared.binary_path else {
            return Ok(ApplyAction::RevealPath(prepared.artifact_path.clone()));
        };
        let current = env::current_exe().map_err(|e| format!("无法定位当前程序路径（{e}）"))?;
        let Some(parent) = current.parent() else {
            return Ok(ApplyAction::RevealPath(new_bin.clone()));
        };
        // Verify install directory is writable before spawning self-replace script.
        if tempfile::NamedTempFile::new_in(parent).is_err() {
            return Ok(ApplyAction::RevealPath(new_bin.clone()));
        }
        let script = env::temp_dir().join(format!("lovelycat-update-{}.cmd", std::process::id()));
        let pid = std::process::id();
        let body = format!(
            "@echo off\r\n\
             setlocal\r\n\
             set \"PID={pid}\"\r\n\
             set \"SRC={}\"\r\n\
             set \"DST={}\"\r\n\
             for /L %%i in (1,1,40) do (\r\n\
               tasklist /FI \"PID eq %PID%\" 2>NUL | find \"%PID%\" >NUL\r\n\
               if errorlevel 1 goto replace\r\n\
               timeout /t 1 /nobreak >NUL\r\n\
             )\r\n\
             :replace\r\n\
             for /L %%i in (1,1,15) do (\r\n\
               copy /Y \"%SRC%\" \"%DST%\" >NUL 2>&1 && goto launch\r\n\
               timeout /t 1 /nobreak >NUL\r\n\
             )\r\n\
             goto end\r\n\
             :launch\r\n\
             start \"\" \"%DST%\"\r\n\
             :end\r\n\
             del \"%~f0\" >NUL 2>&1\r\n",
            new_bin.display(),
            current.display()
        );
        fs::write(&script, body).map_err(|e| format!("无法创建更新脚本（{e}）"))?;
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", &script.to_string_lossy()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(
                windows_sys::Win32::System::Threading::CREATE_NO_WINDOW
                    | windows_sys::Win32::System::Threading::DETACHED_PROCESS,
            );
        }
        cmd.spawn().map_err(|e| format!("无法启动更新脚本（{e}）"))?;
        Ok(ApplyAction::QuitToRestart)
    }
    #[cfg(target_os = "linux")]
    {
        let Some(new_bin) = &prepared.binary_path else {
            return Ok(ApplyAction::RevealPath(prepared.artifact_path.clone()));
        };
        let current = env::current_exe().map_err(|e| format!("无法定位当前程序路径（{e}）"))?;
        let Some(parent) = current.parent() else {
            return Ok(ApplyAction::RevealPath(new_bin.clone()));
        };
        let staged = parent.join(format!(".lovelycat-update-{}", std::process::id()));
        if fs::copy(new_bin, &staged).is_err() {
            return Ok(ApplyAction::RevealPath(new_bin.clone()));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&staged, fs::Permissions::from_mode(0o755));
        }
        if fs::rename(&staged, &current).is_err() {
            let _ = fs::remove_file(&staged);
            return Ok(ApplyAction::RevealPath(new_bin.clone()));
        }
        let _ = Command::new(&current)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        Ok(ApplyAction::QuitToRestart)
    }
    #[cfg(target_os = "macos")]
    {
        Ok(ApplyAction::RevealPath(prepared.artifact_path.clone()))
    }
}

fn extract_binary(artifact: &Path, dir: &Path) -> Result<Option<PathBuf>, String> {
    let name = artifact
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if cfg!(windows) && name.ends_with(".zip") {
        let file = File::open(artifact).map_err(|e| format!("无法打开更新压缩包（{e}）"))?;
        let mut archive =
            zip::ZipArchive::new(file).map_err(|e| format!("无法解析更新压缩包（{e}）"))?;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
            if entry
                .name()
                .rsplit(['/', '\\'])
                .next()
                .is_some_and(|n| n.eq_ignore_ascii_case("lovelycat.exe"))
            {
                let out_path = dir.join("lovelycat.exe");
                let mut out = File::create(&out_path).map_err(|e| e.to_string())?;
                io::copy(&mut entry, &mut out).map_err(|e| format!("解压更新程序失败（{e}）"))?;
                return Ok(Some(out_path));
            }
        }
        return Err("更新压缩包中未找到 lovelycat.exe。".into());
    }
    #[cfg(target_os = "linux")]
    if name.ends_with(".tar.gz") {
        let extract_dir = dir.join("extracted");
        let _ = fs::remove_dir_all(&extract_dir);
        fs::create_dir_all(&extract_dir).map_err(|e| e.to_string())?;
        let status = Command::new("tar")
            .arg("-xzf")
            .arg(artifact)
            .arg("-C")
            .arg(&extract_dir)
            .stdin(Stdio::null())
            .status()
            .map_err(|e| format!("无法解压更新包（{e}）"))?;
        if !status.success() {
            return Err("解压 Linux 更新包失败。".into());
        }
        if let Some(found) = find_file_named(&extract_dir, "lovelycat") {
            return Ok(Some(found));
        }
    }
    Ok(None)
}

#[cfg(target_os = "linux")]
fn find_file_named(root: &Path, target: &str) -> Option<PathBuf> {
    for entry in fs::read_dir(root).ok()?.flatten() {
        let path = entry.path();
        if path.is_file() && path.file_name().and_then(|n| n.to_str()) == Some(target) {
            return Some(path);
        }
        if path.is_dir()
            && let Some(found) = find_file_named(&path, target)
        {
            return Some(found);
        }
    }
    None
}

fn curl_command() -> Command {
    let curl = if cfg!(windows) {
        env::var_os("SystemRoot")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
            .join("System32/curl.exe")
    } else {
        PathBuf::from("curl")
    };
    let mut cmd = Command::new(curl);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    }
    cmd
}

fn fetch_text(url: &str) -> Result<String, String> {
    let mut cmd = curl_command();
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
            "10",
            "--max-time",
            "30",
        ])
        .arg(url)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if !result.status.success() {
        return Err(String::from_utf8_lossy(&result.stderr).trim().to_owned());
    }
    Ok(String::from_utf8_lossy(&result.stdout).into_owned())
}

fn download_file(url: &str, dest: &Path) -> Result<(), String> {
    let mut cmd = curl_command();
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
        .arg(dest)
        .arg(url)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("无法启动下载程序（{e}）"))?;
    if !result.status.success() {
        return Err(format!(
            "下载更新包失败（{}）",
            String::from_utf8_lossy(&result.stderr).trim()
        ));
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, String> {
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
    Ok(hex::encode(hash.finalize()))
}

pub fn format_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024. * 1024.))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.)
    } else {
        format!("{bytes} B")
    }
}

fn normalize_version(v: &str) -> &str {
    v.trim().trim_start_matches(['v', 'V'])
}

pub fn is_newer(latest: &str, current: &str) -> bool {
    let parse = |raw: &str| -> (Vec<u64>, bool) {
        let clean = normalize_version(raw);
        let (core, pre) = clean
            .split_once('-')
            .map_or((clean, false), |(c, _)| (c, true));
        let nums = core
            .split('.')
            .map(|part| part.parse::<u64>().unwrap_or(0))
            .collect();
        (nums, pre)
    };
    let (mut a, a_pre) = parse(latest);
    let (mut b, b_pre) = parse(current);
    let max_len = a.len().max(b.len());
    a.resize(max_len, 0);
    b.resize(max_len, 0);
    match a.cmp(&b) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => !a_pre && b_pre,
    }
}

fn select_asset<'a>(
    assets: &'a [ReleaseAsset],
    os: &str,
    arch: &str,
) -> Option<&'a ReleaseAsset> {
    let preferred: &[&str] = match (os, arch) {
        ("windows", "x86_64") => &["lovelycat-windows-x86_64.zip"],
        ("windows", "aarch64") => &["lovelycat-windows-arm64.zip"],
        ("macos", "aarch64") => &[
            "lovelycat-macos-arm64.dmg",
            "lovelycat-macos-arm64.tar.gz",
        ],
        ("macos", "x86_64") => &[
            "lovelycat-macos-x86_64.dmg",
            "lovelycat-macos-x86_64.tar.gz",
        ],
        ("linux", "x86_64") => &["lovelycat-linux-x86_64.tar.gz"],
        ("linux", "aarch64") => &["lovelycat-linux-arm64.tar.gz"],
        _ => &[],
    };
    for name in preferred {
        if let Some(asset) = assets.iter().find(|a| a.name.eq_ignore_ascii_case(name)) {
            return Some(asset);
        }
    }
    None
}

fn parse_sha256_for_asset(sums: &str, asset_name: &str) -> Option<String> {
    for line in sums.lines() {
        let mut parts = line.split_whitespace();
        let hash = parts.next()?;
        let file = parts.next()?.trim_start_matches('*');
        if file.eq_ignore_ascii_case(asset_name) && hash.len() == 64 {
            return Some(hash.to_owned());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_semver_versions_accurately() {
        assert!(is_newer("v0.0.2", "0.0.1"));
        assert!(is_newer("0.1.0", "0.0.9"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(is_newer("0.0.2", "0.0.2-beta.1"));
        assert!(!is_newer("v0.0.1", "0.0.1"));
        assert!(!is_newer("0.0.1", "0.0.2"));
        assert!(!is_newer("0.0.2-rc.1", "0.0.2"));
    }

    #[test]
    fn selects_matching_platform_asset_and_parses_checksums() {
        let assets = vec![
            ReleaseAsset {
                name: "lovelycat-windows-x86_64.zip".into(),
                download_url: "https://example.invalid/win.zip".into(),
                size: 1024,
            },
            ReleaseAsset {
                name: "lovelycat-macos-arm64.dmg".into(),
                download_url: "https://example.invalid/mac.dmg".into(),
                size: 2048,
            },
            ReleaseAsset {
                name: "lovelycat-linux-x86_64.tar.gz".into(),
                download_url: "https://example.invalid/linux.tar.gz".into(),
                size: 3072,
            },
        ];
        assert_eq!(
            select_asset(&assets, "windows", "x86_64").unwrap().name,
            "lovelycat-windows-x86_64.zip"
        );
        assert_eq!(
            select_asset(&assets, "macos", "aarch64").unwrap().name,
            "lovelycat-macos-arm64.dmg"
        );
        assert_eq!(
            select_asset(&assets, "linux", "x86_64").unwrap().name,
            "lovelycat-linux-x86_64.tar.gz"
        );

        let sums = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  lovelycat-windows-x86_64.zip\n";
        assert_eq!(
            parse_sha256_for_asset(sums, "lovelycat-windows-x86_64.zip").as_deref(),
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
    }
}
