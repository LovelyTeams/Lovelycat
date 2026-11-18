use std::{
    fs::{self, File},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

use crate::{downloads, process};

const WORKER: &str = include_str!("../assets/forensics/worker.py");
const PACKAGES: &str = include_str!("../assets/forensics/runtime.json");
const QUERY_TIMEOUT: Duration = Duration::from_secs(60);
pub(crate) const CANCELLED: &str = "已取消磁盘任务。";

#[derive(Deserialize)]
struct Package {
    url: String,
    sha256: String,
}

pub(crate) fn runtime_path(data: &Path) -> PathBuf {
    data.join("resources/forensics-v1/python.exe")
}

pub(crate) fn install(data: &Path) -> Result<(), String> {
    if !cfg!(all(windows, target_arch = "x86_64")) {
        return Err("这套磁盘取证组件适用于 Windows x64。".into());
    }
    let executable = runtime_path(data);
    if executable.is_file() {
        return expect_ready(&executable);
    }
    let target = executable.parent().expect("runtime directory");
    let resources = target.parent().expect("resources directory");
    fs::create_dir_all(resources).map_err(|e| e.to_string())?;
    let stage = tempfile::tempdir_in(resources).map_err(|e| e.to_string())?;
    let prepared = stage.path().join("runtime");
    let packages_dir = prepared.join("packages");
    fs::create_dir_all(&packages_dir).map_err(|e| e.to_string())?;
    let download = stage.path().join("package.zip");
    let packages: Vec<Package> = serde_json::from_str(PACKAGES).expect("bundled runtime manifest");
    for (index, package) in packages.iter().enumerate() {
        downloads::download(&package.url, &package.sha256, &download)?;
        let archive = File::open(&download).map_err(|e| e.to_string())?;
        let unpack_to = if index == 0 { &prepared } else { &packages_dir };
        zip::ZipArchive::new(archive)
            .and_then(|mut archive| archive.extract(unpack_to))
            .map_err(|e| format!("无法解压磁盘取证组件（{e}）。"))?;
    }
    fs::write(
        prepared.join("python314._pth"),
        "python314.zip\n.\npackages\n",
    )
    .map_err(|e| e.to_string())?;
    expect_ready(&prepared.join("python.exe"))?;
    if let Err(error) = fs::rename(&prepared, target) {
        if executable.is_file() {
            return expect_ready(&executable);
        }
        return Err(format!("无法安装磁盘取证组件（{error}）。"));
    }
    Ok(())
}

fn expect_ready(executable: &Path) -> Result<(), String> {
    let stop = AtomicBool::new(false);
    match run(
        executable,
        &Request::Probe,
        &stop,
        Some(QUERY_TIMEOUT),
        |_| {},
    )? {
        Message::Ready => Ok(()),
        _ => Err("磁盘取证组件无法初始化。".into()),
    }
}

#[derive(Deserialize)]
pub(crate) struct Inventory {
    pub(crate) container: String,
    pub(crate) table: Option<String>,
    pub(crate) size: u64,
    pub(crate) sector_size: u32,
    pub(crate) segments: u32,
    pub(crate) stamp: String,
    pub(crate) volumes: Vec<Volume>,
}

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct Volume {
    pub(crate) number: Option<u32>,
    pub(crate) offset: u64,
    pub(crate) size: u64,
    pub(crate) name: String,
    pub(crate) kind: Encryption,
    pub(crate) methods: Vec<Method>,
    pub(crate) issue: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Encryption {
    Bitlocker,
    Luks1,
    Luks2,
    Unknown,
}

impl Encryption {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Bitlocker => "BitLocker",
            Self::Luks1 => "LUKS1",
            Self::Luks2 => "LUKS2",
            Self::Unknown => "未识别加密格式",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Method {
    Password,
    RecoveryPassword,
    KeyFile,
}

#[derive(Serialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub(crate) enum Credential {
    Password(String),
    RecoveryPassword(String),
    KeyFile(PathBuf),
}

#[derive(Serialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
enum Request<'a> {
    Probe,
    Inspect {
        path: &'a Path,
    },
    Export {
        path: &'a Path,
        stamp: &'a str,
        volume: &'a Volume,
        credential: &'a Credential,
        output: &'a Path,
    },
}

#[derive(Deserialize)]
pub(crate) struct Exported {
    pub(crate) bytes_written: u64,
    pub(crate) sha256: String,
}

#[derive(Clone, Copy, Deserialize)]
pub(crate) struct Progress {
    pub(crate) done: u64,
    pub(crate) total: u64,
}

#[derive(Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
enum Message {
    Ready,
    Inventory(Inventory),
    Exported(Exported),
    Progress(Progress),
    Error(String),
}

pub(crate) fn inspect(data: &Path, path: &Path, stop: &AtomicBool) -> Result<Inventory, String> {
    match run(
        &runtime_path(data),
        &Request::Inspect { path },
        stop,
        Some(QUERY_TIMEOUT),
        |_| {},
    )? {
        Message::Inventory(value) => Ok(value),
        _ => Err("磁盘取证组件未返回分区信息。".into()),
    }
}

pub(crate) struct ExportRequest {
    pub(crate) path: PathBuf,
    pub(crate) stamp: String,
    pub(crate) volume: Volume,
    pub(crate) credential: Credential,
    pub(crate) destination: PathBuf,
}

pub(crate) fn export(
    data: &Path,
    request: ExportRequest,
    stop: &AtomicBool,
    progress: impl FnMut(Progress),
) -> Result<Exported, String> {
    let destination = &request.destination;
    if destination
        .try_exists()
        .map_err(|e| format!("无法检查输出路径（{e}）。"))?
    {
        return Err("输出路径已存在，请选择一个新文件名。".into());
    }
    let directory = destination.parent().ok_or("请选择输出目录。")?;
    let staged = tempfile::Builder::new()
        .prefix(".lovelycat-")
        .suffix(".partial")
        .tempfile_in(directory)
        .map_err(|e| format!("无法在输出目录创建临时文件（{e}）。"))?
        .into_temp_path();
    let result = run(
        &runtime_path(data),
        &Request::Export {
            path: &request.path,
            stamp: &request.stamp,
            volume: &request.volume,
            credential: &request.credential,
            output: &staged,
        },
        stop,
        None,
        progress,
    )?;
    let Message::Exported(result) = result else {
        return Err("磁盘取证组件未返回导出结果。".into());
    };
    let length = fs::metadata(&staged).map_err(|e| e.to_string())?.len();
    if length == 0
        || length != result.bytes_written
        || result.sha256.len() != 64
        || !result.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("解密镜像的长度或校验记录不完整。".into());
    }
    if stop.load(Ordering::Relaxed) {
        return Err(CANCELLED.into());
    }
    staged
        .persist_noclobber(destination)
        .map_err(|e| format!("无法保存副本，请检查目录权限并选择新的文件名（{e}）。"))?;
    Ok(result)
}

fn run(
    executable: &Path,
    request: &Request<'_>,
    stop: &AtomicBool,
    timeout: Option<Duration>,
    mut progress: impl FnMut(Progress),
) -> Result<Message, String> {
    if stop.load(Ordering::Relaxed) {
        return Err(CANCELLED.into());
    }
    if !executable.is_file() {
        return Err("请先安装磁盘取证组件。".into());
    }
    let mut command = process::command(executable);
    command
        .args(["-I", "-B", "-u", "-c", WORKER])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut process = process::ManagedChild::spawn(&mut command)
        .map_err(|e| format!("无法启动磁盘取证组件（{e}）。"))?;
    let stdin = process.child.stdin.take().expect("piped stdin");
    // The credential is sent through this pipe. The worker reports only sanitized messages.
    serde_json::to_writer(stdin, request).map_err(|_| "无法向磁盘取证组件传递请求。")?;
    let stdout = process.child.stdout.take().expect("piped stdout");
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(serde_json::from_str::<Message>(&line)).is_err() {
                return;
            }
        }
    });
    let started = Instant::now();
    let interval = Duration::from_millis(80);
    let result = loop {
        if stop.load(Ordering::Relaxed) {
            return Err(CANCELLED.into());
        }
        if timeout.is_some_and(|timeout| started.elapsed() >= timeout) {
            return Err("读取镜像或初始化组件超时。".into());
        }
        match rx.recv_timeout(interval) {
            Ok(Ok(Message::Progress(value))) => progress(value),
            Ok(Ok(Message::Error(error))) => return Err(error),
            Ok(Ok(done)) => break done,
            Ok(Err(_)) => return Err("磁盘取证组件返回了无法读取的数据。".into()),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("磁盘取证组件提前退出，未完成操作。".into());
            }
        }
    };
    let status = process.child.wait().map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("磁盘取证组件退出异常。".into());
    }
    Ok(result)
}

#[cfg(all(test, windows, target_arch = "x86_64"))]
mod tests {
    use super::*;

    #[test]
    #[ignore = "downloads the pinned worker and a published synthetic LUKS2 fixture"]
    fn exports_volume_and_keeps_sources_and_existing_files_intact() {
        let sandbox = tempfile::tempdir().unwrap();
        let data = sandbox.path();
        install(data).unwrap();

        let archive = data.join("luks.gz");
        downloads::download(
            "https://raw.githubusercontent.com/fox-it/dissect.fve/4.6/tests/_data/luks2/aes-xts-plain64.bin.gz",
            "2aab1e539dfc4616fb4657320a2ce65336eb7dbcb7d4c3d9981a0f2033495f66",
            &archive,
        )
        .unwrap();
        let source = data.join("取证 附件.dd");
        std::io::copy(
            &mut flate2::read::GzDecoder::new(File::open(archive).unwrap()),
            &mut File::create(&source).unwrap(),
        )
        .unwrap();
        let original = fs::read(&source).unwrap();
        let stop = AtomicBool::new(false);
        let schema = inspect(data, &source, &stop).unwrap();
        let volume = &schema.volumes[0];
        assert!(volume.kind == Encryption::Luks2);
        let request = |path: &Path| ExportRequest {
            path: source.clone(),
            stamp: schema.stamp.clone(),
            volume: volume.clone(),
            credential: Credential::Password("password".into()),
            destination: path.into(),
        };

        let finished = data.join("解密副本.img");
        let result = export(data, request(&finished), &stop, |_| {}).unwrap();
        assert_eq!(result.bytes_written, 2048);
        assert_eq!(
            fs::read(finished).unwrap(),
            (0..4_u8).flat_map(|byte| [byte; 512]).collect::<Vec<_>>()
        );
        assert!(export(data, request(&source), &stop, |_| {}).is_err());
        assert_eq!(fs::read(&source).unwrap(), original);

        let claimed = data.join("existing.img");
        assert!(
            export(data, request(&claimed), &stop, |_| fs::write(
                &claimed,
                b"existing"
            )
            .unwrap())
            .is_err()
        );
        assert_eq!(fs::read(claimed).unwrap(), b"existing");

        let cancelled = data.join("cancelled.img");
        let failure = export(data, request(&cancelled), &stop, |_| {
            stop.store(true, Ordering::Relaxed);
        });
        assert!(matches!(failure, Err(message) if message == CANCELLED));
        assert!(!cancelled.exists());
        assert!(
            !fs::read_dir(data)
                .unwrap()
                .flatten()
                .any(|entry| { entry.path().extension().is_some_and(|ext| ext == "partial") })
        );
    }
}
