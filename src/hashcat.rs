use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

use crate::process::{Cancel, ManagedChild, command};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

const RESULT_NAME: &str = "recovered.txt";
const PREVIEW_BYTES: u64 = 64 * 1024;

#[derive(Clone, Debug, Deserialize)]
pub struct Mode {
    #[serde(rename = "hash_mode")]
    pub id: u32,
    pub name: String,
    #[serde(default)]
    pub category: String,
}

pub struct Installation {
    pub version: String,
    pub modes: Vec<Mode>,
    pub attacks: Vec<AttackMode>,
    pub options: Vec<CliOption>,
}

pub struct CliOption {
    pub flags: String,
    pub flag: String,
    pub kind: String,
    pub description: String,
}

pub struct AttackMode {
    pub id: u32,
    pub name: String,
}

impl AttackMode {
    pub fn label(&self) -> &str {
        match self.id {
            0 => "字典",
            1 => "双字典组合",
            3 => "掩码",
            4 => "PCFG 语法",
            5 => "表格变换",
            6 => "字典 + 掩码",
            7 => "掩码 + 字典",
            8 => "通用候选源（Feed）",
            9 => "逐条关联",
            12 => "字典占位掩码",
            _ => &self.name,
        }
    }
}

pub enum HashInput {
    File(PathBuf),
    Pasted(String),
}

pub enum Candidates {
    Dictionary {
        words: PathBuf,
        rule: Option<PathBuf>,
    },
    Mask(OsString),
    Combination {
        left: PathBuf,
        right: PathBuf,
    },
    Hybrid {
        words: PathBuf,
        mask: OsString,
        mask_first: bool,
    },
    Positional {
        attack: u32,
        operands: Vec<OsString>,
        rule: Option<PathBuf>,
    },
}

pub struct Request {
    pub input: HashInput,
    pub mode: Option<u32>,
    pub candidates: Candidates,
    pub username: bool,
    pub source_name: String,
    pub password_only: bool,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Status {
    pub progress: Option<[u64; 2]>,
    pub recovered_hashes: Option<[u64; 2]>,
    #[serde(default)]
    pub devices: Vec<DeviceStatus>,
    pub estimated_stop: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct DeviceStatus {
    pub speed: u64,
}

pub enum RunEvent {
    ResultPath(PathBuf),
    Passwords(Vec<String>),
    Log(String),
    Status(Status),
    Finished(Result<Outcome, String>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Recovered,
    Exhausted,
    Stopped,
}

pub enum CommandEvent {
    Line(String),
    Finished(Result<Option<i32>, String>), // None when stopped by the user.
}

/// Each line is one literal argv element, including any spaces in a file path.
pub fn argument_lines(text: &str) -> Vec<OsString> {
    text.lines()
        .filter(|line| !line.is_empty())
        .map(OsString::from)
        .collect()
}

pub fn start_command(
    executable: PathBuf,
    args: Vec<OsString>,
    terminal: bool,
    stdin_file: Option<PathBuf>,
    stdout_file: Option<PathBuf>,
) -> (Cancel, async_channel::Receiver<CommandEvent>) {
    let (cancel, flag) = Cancel::pair();
    let (tx, rx) = async_channel::bounded(256);
    thread::spawn(move || {
        let mut cmd = command(&executable);
        cmd.args(args);
        let exit = if terminal {
            run_in_terminal(&mut cmd, &flag)
        } else {
            run_with_streams(
                &mut cmd,
                &flag,
                stdin_file.as_deref(),
                stdout_file.as_deref(),
                |line| {
                    let _ = tx.send_blocking(CommandEvent::Line(line));
                },
            )
        };
        let outcome = exit.and_then(|exit| match exit {
            Exit::Stopped => Ok(None),
            Exit::Finished(status) => status
                .code()
                .map(Some)
                .ok_or_else(|| "进程未返回退出代码。".into()),
        });
        let _ = tx.send_blocking(CommandEvent::Finished(outcome));
    });
    (cancel, rx)
}

fn run_with_streams(
    cmd: &mut Command,
    stop: &AtomicBool,
    input: Option<&Path>,
    output: Option<&Path>,
    on_line: impl FnMut(String),
) -> Result<Exit, String> {
    let stdin = match input {
        Some(path) => {
            Stdio::from(File::open(path).map_err(|e| format!("无法打开标准输入文件（{e}）"))?)
        }
        None => Stdio::null(),
    };
    if let Some(path) = output {
        let directory = path.parent().ok_or("输出路径缺少父目录。")?;
        let file = tempfile::NamedTempFile::new_in(directory)
            .map_err(|e| format!("无法创建输出文件（{e}）"))?;
        let stream = file.reopen().map_err(|e| e.to_string())?;
        let exit = execute_io(cmd, Some(stop), None, stdin, Stdio::from(stream), on_line)?;
        file.persist(path)
            .map_err(|e| format!("无法保存标准输出（{e}）"))?;
        Ok(exit)
    } else {
        execute_io(cmd, Some(stop), None, stdin, Stdio::piped(), on_line)
    }
}

fn run_in_terminal(cmd: &mut Command, stop: &AtomicBool) -> Result<Exit, String> {
    #[cfg(windows)]
    cmd.creation_flags(windows_sys::Win32::System::Threading::CREATE_NEW_CONSOLE);
    let mut process =
        ManagedChild::spawn(cmd).map_err(|e| format!("无法打开 Hashcat 终端（{e}）"))?;
    loop {
        if stop.load(Ordering::Relaxed) {
            return Ok(Exit::Stopped);
        }
        if let Some(status) = process.child.try_wait().map_err(|e| e.to_string())? {
            return Ok(Exit::Finished(status));
        }
        thread::sleep(Duration::from_millis(80));
    }
}

#[derive(Clone)]
pub struct ResultFile {
    pub path: PathBuf,
    pub modified: SystemTime,
    pub source: String,
}

#[derive(Default, Deserialize, Serialize)]
struct Settings {
    executable: Option<PathBuf>,
}

pub fn data_dir() -> PathBuf {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir)
        .join("Lovelycat")
}

#[derive(Default, Serialize, Deserialize)]
pub struct ResultLocations {
    // Most recently selected first. Keeping old locations preserves the results list.
    roots: Vec<PathBuf>,
}

impl ResultLocations {
    pub fn load(data: &Path) -> Self {
        fs::read(data.join("result-locations.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn active(&self, data: &Path) -> PathBuf {
        self.roots
            .first()
            .cloned()
            .unwrap_or_else(|| data.join("jobs"))
    }

    pub fn select(&mut self, directory: PathBuf, data: &Path) -> io::Result<()> {
        fs::create_dir_all(&directory)?;
        let _writable = tempfile::NamedTempFile::new_in(&directory)?;
        fs::create_dir_all(data)?;
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(data.join("result-locations.lock"))?;
        lock.lock()?;
        let existing = Self::load(data);
        let next = Self {
            roots: std::iter::once(directory.clone())
                .chain(existing.roots.into_iter().filter(|p| *p != directory))
                .collect(),
        };
        let mut file = tempfile::NamedTempFile::new_in(data)?;
        serde_json::to_writer(&mut file, &next)?;
        file.flush()?;
        file.persist(data.join("result-locations.json"))?;
        *self = next;
        Ok(())
    }
}

pub fn locate(data: &Path) -> Option<PathBuf> {
    if let Some(saved) = fs::read(data.join("settings.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Settings>(&bytes).ok())
        .and_then(|settings| settings.executable)
        .filter(|path| path.is_absolute() && path.is_file())
    {
        return Some(saved);
    }

    let cwd = env::current_dir().ok();
    let downloaded = data.join("resources/hashcat-8.0.0/hashcat.exe");
    if downloaded.is_file() {
        return Some(downloaded);
    }
    let exe = env::current_exe().ok();
    let search_dirs = cwd.iter().map(PathBuf::as_path).chain(
        exe.as_deref()
            .and_then(Path::parent)
            .into_iter()
            .flat_map(|path| path.ancestors().take(4)),
    );
    for dir in search_dirs {
        for relative in ["hashcat-8.0.0/hashcat.exe", "hashcat.exe"] {
            let path = dir.join(relative);
            if path.is_file() {
                return Some(path);
            }
        }
    }

    env::var_os("PATH").and_then(|path| {
        env::split_paths(&path)
            .filter(|dir| dir.is_absolute())
            .map(|dir| dir.join("hashcat.exe"))
            .find(|exe| exe.is_file())
    })
}

pub fn save_executable(data: &Path, executable: PathBuf) -> io::Result<()> {
    fs::create_dir_all(data)?;
    let mut file = tempfile::NamedTempFile::new_in(data)?;
    serde_json::to_writer(
        &mut file,
        &Settings {
            executable: Some(executable),
        },
    )?;
    file.flush()?;
    file.persist(data.join("settings.json"))?;
    Ok(())
}

pub fn probe(executable: &Path) -> Result<Installation, String> {
    if !executable.is_absolute() || !executable.is_file() {
        return Err("所选 Hashcat 可执行文件不存在。".into());
    }
    let output = capture(
        command(executable).arg("--version"),
        Duration::from_secs(15),
    )?;
    let version = output
        .lines()
        .map(str::trim)
        .find(|line| {
            line.strip_prefix('v')
                .and_then(|version| version.split_once('.'))
                .and_then(|(major, _)| major.parse::<u32>().ok())
                .is_some_and(|major| major >= 8)
        })
        .ok_or("请选择 Hashcat 8.0 或更新版本的可执行文件。")?
        .to_owned();

    let help = capture(command(executable).arg("-hh"), Duration::from_secs(25))?;
    let installation = parse_help(version, &help);
    if installation.modes.is_empty()
        || installation.attacks.is_empty()
        || installation.options.is_empty()
    {
        return Err("无法读取 Hashcat 的模式与选项，请检查安装目录。".into());
    }
    Ok(installation)
}

fn parse_help(version: String, help: &str) -> Installation {
    let mut section = "";
    let mut modes = Vec::new();
    let mut attacks = Vec::new();
    let mut options = Vec::new();
    for line in help.lines().map(str::trim) {
        if let Some(title) = line
            .strip_prefix("- [ ")
            .and_then(|s| s.strip_suffix(" ] -"))
        {
            section = title;
            continue;
        }
        let Some((first, rest)) = line.split_once('|') else {
            continue;
        };
        match section {
            "Hash Modes" => {
                if let (Ok(id), Some((name, category))) =
                    (first.trim().parse(), rest.rsplit_once('|'))
                {
                    modes.push(Mode {
                        id,
                        name: name.trim().into(),
                        category: category.trim().into(),
                    });
                }
            }
            "Attack Modes" => {
                if let Ok(id) = first.trim().parse() {
                    attacks.push(AttackMode {
                        id,
                        name: rest.trim().into(),
                    });
                }
            }
            "Options" if first.starts_with('-') => {
                let flags = first.trim();
                let mut cells = rest.split('|').map(str::trim);
                if let (Some(kind), Some(description)) = (cells.next(), cells.next()) {
                    options.push(CliOption {
                        flags: flags.into(),
                        flag: flags.rsplit(',').next().unwrap_or(flags).trim().into(),
                        kind: kind.into(),
                        description: description.into(),
                    });
                }
            }
            _ => {}
        }
    }
    modes.sort_by_key(|mode| mode.id);
    modes.dedup_by_key(|mode| mode.id);
    Installation {
        version,
        modes,
        attacks,
        options,
    }
}

pub fn devices(executable: &Path) -> Result<String, String> {
    capture(
        command(executable).args(["-I", "--logfile-disable"]),
        Duration::from_secs(45),
    )
}

pub fn identify(executable: &Path, input: HashInput, username: bool) -> Result<Vec<Mode>, String> {
    let input = PreparedInput::new(input)?;
    let mut args = command(executable);
    args.args([
        "--identify",
        "--json",
        "--quiet",
        "--logfile-disable",
        "--potfile-disable",
    ]);
    if username {
        args.arg("--username");
    }
    args.arg("--").arg(&input.path);

    #[derive(Deserialize)]
    struct Matches {
        modes: Vec<Mode>,
    }
    let text = capture(&mut args, Duration::from_secs(45))?;
    text.lines()
        .find_map(|line| serde_json::from_str::<Matches>(line.trim()).ok())
        .map(|matches| matches.modes)
        .ok_or_else(|| "Hashcat 没有返回可识别的哈希类型。".into())
}

pub fn start(
    executable: PathBuf,
    data: PathBuf,
    request: Request,
) -> (Cancel, async_channel::Receiver<RunEvent>) {
    let (cancel, flag) = Cancel::pair();
    let (tx, rx) = async_channel::bounded(256);
    thread::spawn(move || {
        let outcome = run(&executable, &data, request, &flag, &tx);
        let _ = tx.send_blocking(RunEvent::Finished(outcome));
    });
    (cancel, rx)
}

fn run(
    executable: &Path,
    data: &Path,
    request: Request,
    stop: &AtomicBool,
    events: &async_channel::Sender<RunEvent>,
) -> Result<Outcome, String> {
    let input = PreparedInput::new(request.input)?;
    let jobs = ResultLocations::load(data).active(data);
    let cache = data.join("cache");
    fs::create_dir_all(&jobs)
        .and_then(|_| fs::create_dir_all(&cache))
        .map_err(|e| format!("无法创建 Lovelycat 数据目录（{e}）"))?;
    let job = tempfile::Builder::new()
        .prefix("run-")
        .tempdir_in(&jobs)
        .map_err(|e| format!("无法创建任务目录（{e}）"))?;
    let result = job.path().join(RESULT_NAME);
    let _ = events.send_blocking(RunEvent::ResultPath(result.clone()));

    let mut cmd = command(executable);
    cmd.args([
        "--status",
        "--status-json",
        "--status-timer",
        "1",
        "--potfile-disable",
        "--restore-disable",
        "--logfile-disable",
        "--outfile-format",
    ])
    .arg(if request.password_only { "2" } else { "1,2" })
    .arg("--cache-path")
    .arg(&cache)
    .arg("--outfile")
    .arg(&result);
    if let Some(mode) = request.mode {
        cmd.args(["-m", &mode.to_string()]);
    }
    if request.username {
        cmd.arg("--username");
    }
    let attack_mode = match &request.candidates {
        Candidates::Dictionary { rule, .. } => {
            if let Some(rule) = rule {
                cmd.arg("-r").arg(rule);
            }
            0
        }
        Candidates::Combination { .. } => 1,
        Candidates::Mask(_) => 3,
        Candidates::Hybrid {
            mask_first: false, ..
        } => 6,
        Candidates::Hybrid {
            mask_first: true, ..
        } => 7,
        Candidates::Positional { attack, rule, .. } => {
            if let Some(path) = rule {
                cmd.arg("-r").arg(path);
            }
            *attack
        }
    };
    cmd.arg("-a")
        .arg(attack_mode.to_string())
        .arg("--")
        .arg(&input.path);
    match &request.candidates {
        Candidates::Dictionary { words, .. } => cmd.arg(words),
        Candidates::Mask(mask) => cmd.arg(mask),
        Candidates::Combination { left, right } => cmd.arg(left).arg(right),
        Candidates::Hybrid {
            words,
            mask,
            mask_first: false,
        } => cmd.arg(words).arg(mask),
        Candidates::Hybrid {
            words,
            mask,
            mask_first: true,
        } => cmd.arg(mask).arg(words),
        Candidates::Positional { operands, .. } => cmd.args(operands),
    };

    let mut password_offset = 0;
    let mut publish_passwords = || {
        let _ = read_new_passwords(
            &result,
            &mut password_offset,
            request.password_only,
            |batch| {
                let _ = events.send_blocking(RunEvent::Passwords(batch));
            },
        );
    };
    let exit = execute(&mut cmd, Some(stop), None, |line| {
        let event = if line.starts_with('{') {
            match serde_json::from_str::<Status>(&line) {
                Ok(status) if status.progress.is_some() => {
                    if status.recovered_hashes.is_some_and(|[n, _]| n > 0) {
                        publish_passwords();
                    }
                    RunEvent::Status(status)
                }
                _ => RunEvent::Log(line),
            }
        } else if line.starts_with("[s]tatus") {
            return;
        } else {
            RunEvent::Log(line)
        };
        let _ = events.send_blocking(event);
    });

    if has_results(&result) {
        publish_passwords();
        let _ = fs::write(job.path().join("source.txt"), &request.source_name);
        let _ = job.keep();
    }
    match exit? {
        Exit::Stopped => Ok(Outcome::Stopped),
        Exit::Finished(status) => match status.code() {
            Some(0) => Ok(Outcome::Recovered),
            Some(1) => Ok(Outcome::Exhausted),
            code => Err(format!(
                "Hashcat 退出（代码 {}），请查看运行日志。",
                code.map_or_else(|| "未知".into(), |code| code.to_string())
            )),
        },
    }
}

pub fn has_results(path: &Path) -> bool {
    path.metadata()
        .is_ok_and(|meta| meta.is_file() && meta.len() > 0)
}

pub fn results(data: &Path) -> Vec<ResultFile> {
    let mut roots = ResultLocations::load(data).roots;
    let default = data.join("jobs");
    if !roots.contains(&default) {
        roots.push(default);
    }
    let mut files: Vec<_> = roots
        .into_iter()
        .filter_map(|root| fs::read_dir(root).ok())
        .flatten()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("run-"))
        .filter_map(|entry| {
            let path = entry.path().join(RESULT_NAME);
            let meta = path.metadata().ok()?;
            (meta.is_file() && meta.len() > 0).then(|| ResultFile {
                modified: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                source: fs::read_to_string(entry.path().join("source.txt"))
                    .unwrap_or_else(|_| "Hashcat 任务".into()),
                path,
            })
        })
        .collect();
    files.sort_by(|left, right| right.modified.cmp(&left.modified));
    files
}

fn read_new_passwords(
    path: &Path,
    offset: &mut u64,
    plain_only: bool,
    mut publish: impl FnMut(Vec<String>),
) -> io::Result<()> {
    let mut reader = BufReader::new(File::open(path)?);
    reader.seek(SeekFrom::Start(*offset))?;
    let mut batch = Vec::new();
    let mut row = Vec::new();
    loop {
        row.clear();
        let bytes = reader.read_until(b'\n', &mut row)?;
        // Hashcat may be midway through appending a record. Re-read it on the next update.
        if bytes == 0 || !row.ends_with(b"\n") {
            break;
        }
        *offset += bytes as u64;
        let plain = row.strip_suffix(b"\n").unwrap_or(&row);
        let plain = plain.strip_suffix(b"\r").unwrap_or(plain);
        // The default outfile encoding quotes separator-containing passwords as $HEX[...].
        let plain = if plain_only {
            plain
        } else {
            plain.rsplit(|b| *b == b':').next().unwrap_or(plain)
        };
        let decoded = plain
            .strip_prefix(b"$HEX[")
            .and_then(|v| v.strip_suffix(b"]"))
            .and_then(|v| hex::decode(v).ok());
        let text = decoded
            .as_deref()
            .and_then(|v| std::str::from_utf8(v).ok())
            .filter(|s| !s.chars().any(char::is_control))
            .map(str::to_owned)
            .unwrap_or_else(|| String::from_utf8_lossy(plain).into_owned());
        batch.push(text);
        if batch.len() == 256 {
            publish(std::mem::take(&mut batch));
        }
    }
    if !batch.is_empty() {
        publish(batch);
    }
    Ok(())
}

pub fn preview(path: &Path) -> io::Result<(String, bool)> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(PREVIEW_BYTES + 1)
        .read_to_end(&mut bytes)?;
    let truncated = bytes.len() as u64 > PREVIEW_BYTES;
    if truncated {
        bytes.truncate(PREVIEW_BYTES as usize);
        if let Some(last_newline) = bytes.iter().rposition(|byte| *byte == b'\n') {
            bytes.truncate(last_newline + 1);
        }
    }
    Ok((String::from_utf8_lossy(&bytes).into_owned(), truncated))
}

struct PreparedInput {
    path: PathBuf,
    _temp: Option<tempfile::TempDir>,
}

impl PreparedInput {
    fn new(input: HashInput) -> Result<Self, String> {
        match input {
            HashInput::File(path) => Ok(Self { path, _temp: None }),
            HashInput::Pasted(text) => {
                let temp =
                    tempfile::tempdir().map_err(|e| format!("无法创建哈希临时目录（{e}）"))?;
                let path = temp.path().join("hashes.txt");
                let mut file =
                    File::create(&path).map_err(|e| format!("无法创建哈希临时文件（{e}）"))?;
                file.write_all(text.as_bytes())
                    .and_then(|_| {
                        if text.ends_with('\n') {
                            Ok(())
                        } else {
                            file.write_all(b"\n")
                        }
                    })
                    .map_err(|e| format!("无法写入哈希临时文件（{e}）"))?;
                Ok(Self {
                    path,
                    _temp: Some(temp),
                })
            }
        }
    }
}

fn capture(command: &mut Command, timeout: Duration) -> Result<String, String> {
    let mut output = String::new();
    let exit = execute(command, None, Some(timeout), |line| {
        if output.len() + line.len() < 1024 * 1024 {
            output.push_str(&line);
            output.push('\n');
        }
    })?;
    if matches!(exit, Exit::Finished(status) if status.success()) {
        return Ok(output);
    }
    let details: Vec<_> = output.lines().rev().take(6).collect();
    let diagnostic = details.into_iter().rev().collect::<Vec<_>>().join("\n");
    Err(if diagnostic.is_empty() {
        "Hashcat 未能完成查询。".into()
    } else {
        diagnostic
    })
}

enum Exit {
    Finished(ExitStatus),
    Stopped,
}

fn execute(
    command: &mut Command,
    stop: Option<&AtomicBool>,
    timeout: Option<Duration>,
    on_line: impl FnMut(String),
) -> Result<Exit, String> {
    execute_io(
        command,
        stop,
        timeout,
        Stdio::null(),
        Stdio::piped(),
        on_line,
    )
}

fn execute_io(
    command: &mut Command,
    stop: Option<&AtomicBool>,
    timeout: Option<Duration>,
    stdin: Stdio,
    stdout: Stdio,
    mut on_line: impl FnMut(String),
) -> Result<Exit, String> {
    let is_stopped = || stop.is_some_and(|stop| stop.load(Ordering::Relaxed));
    if is_stopped() {
        return Ok(Exit::Stopped);
    }
    command.stdin(stdin).stdout(stdout).stderr(Stdio::piped());
    let mut process =
        ManagedChild::spawn(command).map_err(|e| format!("无法启动 Hashcat（{e}）"))?;
    let (tx, rx) = mpsc::sync_channel(256);
    if let Some(pipe) = process.child.stdout.take() {
        pipe_lines(pipe, tx.clone());
    }
    pipe_lines(process.child.stderr.take().expect("piped stderr"), tx);
    let started = Instant::now();
    let exit = loop {
        match process.child.try_wait() {
            Ok(Some(status)) => break Ok(Exit::Finished(status)),
            Ok(None) => {}
            Err(error) => break Err(format!("无法读取 Hashcat 进程状态（{error}）")),
        }
        if is_stopped() {
            break Ok(Exit::Stopped);
        }
        if timeout.is_some_and(|timeout| started.elapsed() >= timeout) {
            break Err("Hashcat 查询超时，请检查驱动和运行环境后重试。".into());
        }
        let interval = Duration::from_millis(40);
        match rx.recv_timeout(interval) {
            Ok(line) => on_line(line),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => thread::sleep(interval),
        }
    };
    // Close the job first so that a child retaining a pipe cannot block draining it.
    drop(process);
    for line in rx {
        on_line(line);
    }
    exit
}

fn pipe_lines(pipe: impl Read + Send + 'static, tx: mpsc::SyncSender<String>) {
    thread::spawn(move || {
        let mut reader = BufReader::new(pipe);
        let mut bytes = Vec::new();
        loop {
            bytes.clear();
            match reader.read_until(b'\n', &mut bytes) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    for line in String::from_utf8_lossy(&bytes)
                        .trim_end_matches(['\n', '\r'])
                        .split('\r')
                    {
                        if !line.is_empty() && tx.send(line.to_owned()).is_err() {
                            return;
                        }
                    }
                }
            }
        }
    });
}

pub fn file_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or_else(|| OsStr::new("文件"))
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_keeps_categories_aliases_and_new_engine_modes() {
        let help = "- [ Options ] -\n -ii,--increment-inverse | | Reverse |\n --sample | File | Read data | sample\n- [ Hash Modes ] -\n 501 | Name | with pipe | Documents\n 501 | Duplicate | Documents\n- [ Attack Modes ] -\n 12 | Hybrid\n 42 | Future generator\n- [ Outfile Formats ] -\n 5 | Unrelated";
        let installed = parse_help("v8".into(), help);
        assert_eq!(installed.modes.len(), 1);
        assert_eq!(installed.modes[0].name, "Name | with pipe");
        assert_eq!(installed.modes[0].category, "Documents");
        assert_eq!(installed.options[0].flag, "--increment-inverse");
        assert_eq!(installed.options[1].kind, "File");
        assert_eq!(installed.attacks[1].label(), "Future generator");
    }

    #[test]
    fn command_arguments_preserve_literal_text_and_windows_paths() {
        assert_eq!(
            argument_lines("-m\r\n0\r\nC:\\my files\\x\n  x&y | z  \n"),
            ["-m", "0", "C:\\my files\\x", "  x&y | z  "].map(OsString::from)
        );
    }

    #[test]
    fn switching_directories_preserves_old_and_new_history() {
        let data = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        for root in [data.path().join("jobs"), other.path().into()] {
            let job = root.join("run-123");
            fs::create_dir_all(&job).unwrap();
            fs::write(job.join(RESULT_NAME), "fixture").unwrap();
        }
        let mut locations = ResultLocations::load(data.path());
        locations.select(other.path().into(), data.path()).unwrap();
        assert_eq!(
            ResultLocations::load(data.path()).active(data.path()),
            other.path()
        );
        locations
            .select(data.path().join("jobs"), data.path())
            .unwrap();
        assert_eq!(results(data.path()).len(), 2);
    }

    #[test]
    fn inline_results_preserve_empty_passwords_colons_unicode_and_partial_appends() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(b"hash:salt:$HEX[613a62]\r\nhash:\r\nhash:half")
            .unwrap();
        let mut offset = 0;
        let mut values = Vec::new();
        read_new_passwords(file.path(), &mut offset, false, |rows| values.extend(rows)).unwrap();
        assert_eq!(values, ["a:b", ""]);
        writeln!(file, " 猫").unwrap();
        read_new_passwords(file.path(), &mut offset, false, |rows| values.extend(rows)).unwrap();
        assert_eq!(values, ["a:b", "", "half 猫"]);
        fs::write(file.path(), b"plain:tail\n").unwrap();
        read_new_passwords(file.path(), &mut 0, true, |rows| {
            assert_eq!(rows, ["plain:tail"])
        })
        .unwrap();
    }
}
