mod forensics;
mod helpers;
mod layout;
mod palette;
mod settings;
mod tools;
pub use palette::configure_theme;

use std::{
    collections::{HashMap, VecDeque},
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

use gpui::{
    App, AppContext, Context, Entity, PathPromptOptions, ScrollHandle, SharedString, Window, px,
};
use gpui_component::{
    IndexPath,
    input::InputState,
    select::{SearchableVec, SelectEvent, SelectItem, SelectState},
};

use self::tools::EngineTools;
use crate::desktop::{Appearance, Outline, Preferences, WindowOutline};
use crate::hashcat::{
    self, Candidates, HashInput, Installation, Mode, Outcome, Request, ResultFile, RunEvent, Status,
};
use crate::process::Cancel;
use crate::protected::{self, Extracted};

const LOG_LIMIT: usize = 400;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Task,
    Results,
    Tools,
    Forensics,
    Settings,
}

#[derive(Clone, Copy)]
enum FileSlot {
    Protected,
    Hashes,
    Words,
    SecondWords,
    Rule,
    Mask,
    OperandFile,
    OperandDirectory,
    ArgumentFile,
    ArgumentDirectory,
    Stdin,
    ResultsDirectory,
    Executable,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Source {
    Protected,
    HashFile,
    Paste,
}

enum Extraction {
    Idle,
    Pending,
    Ready(Extracted),
    Failed(String),
}

impl FileSlot {
    fn prompt(self) -> &'static str {
        match self {
            Self::Protected => "选择受密码保护的文件",
            Self::Hashes => "选择哈希文件",
            Self::Words => "选择字典文件",
            Self::SecondWords => "选择第二份字典",
            Self::Rule => "选择规则文件",
            Self::Mask => "选择掩码文件",
            Self::OperandFile | Self::ArgumentFile => "添加文件路径",
            Self::OperandDirectory | Self::ArgumentDirectory => "添加目录路径",
            Self::Stdin => "选择标准输入文件",
            Self::ResultsDirectory => "选择恢复结果目录",
            Self::Executable => "选择 hashcat.exe",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Ready,
    Starting,
    Running,
    Stopping,
    Recovered,
    Exhausted,
    Stopped,
    Failed,
}

impl Phase {
    fn label(self) -> &'static str {
        match self {
            Self::Ready => "待运行",
            Self::Starting => "正在启动",
            Self::Running => "运行中",
            Self::Stopping => "正在停止",
            Self::Recovered => "恢复完成",
            Self::Exhausted => "候选已用完",
            Self::Stopped => "已停止",
            Self::Failed => "运行失败",
        }
    }
}

enum DeviceQuery {
    Idle,
    Pending,
    Ready(String),
    Failed(String),
}

struct Notice {
    text: String,
    error: bool,
}

#[derive(Clone)]
struct ModeChoice {
    id: Option<u32>,
    label: SharedString,
}

impl SelectItem for ModeChoice {
    type Value = Option<u32>;

    fn title(&self) -> SharedString {
        self.label.clone()
    }
    fn value(&self) -> &Self::Value {
        &self.id
    }
}

impl From<&Mode> for ModeChoice {
    fn from(mode: &Mode) -> Self {
        Self {
            id: Some(mode.id),
            label: format!("{}  ·  {}", mode.id, mode.name).into(),
        }
    }
}

fn auto_mode() -> ModeChoice {
    ModeChoice {
        id: None,
        label: "自动识别".into(),
    }
}

pub struct Workbench {
    page: Page,
    data: PathBuf,
    executable: Option<PathBuf>,
    installation: Option<Installation>,
    probing: bool,
    engine_notice: Option<Notice>,
    devices: DeviceQuery,
    picking: bool,
    preferences: Preferences,
    outline: WindowOutline,
    locations: hashcat::ResultLocations,
    download: Option<crate::downloads::Resource>,
    download_note: Option<Notice>,
    helpers: helpers::QuickSetup,
    forensics: forensics::Forensics,
    appearance_error: Option<String>,

    source: Source,
    protected_file: Option<PathBuf>,
    extraction: Extraction,
    hashes: Option<PathBuf>,
    hash_text: Entity<InputState>,
    modes: Entity<SelectState<SearchableVec<ModeChoice>>>,
    identifying: bool,
    type_notice: Option<Notice>,
    username: bool,
    attacks: Entity<SelectState<SearchableVec<ModeChoice>>>,
    attack: u32,
    operands: Entity<InputState>,
    operand_drafts: HashMap<u32, String>,
    words: Option<PathBuf>,
    second_words: Option<PathBuf>,
    rule: Option<PathBuf>,
    mask_file: Option<PathBuf>,
    mask_text: Entity<InputState>,
    form_error: Option<String>,
    tools: EngineTools,

    cancel: Option<Cancel>,
    phase: Phase,
    status: Status,
    logs: VecDeque<SharedString>,
    log_scroll: ScrollHandle,
    current_result: Option<PathBuf>,
    found_passwords: Vec<String>,
    run_error: Option<String>,
    copied_log: bool,

    results: Vec<ResultFile>,
    selected_result: Option<PathBuf>,
    result_preview: Option<Result<(String, bool), String>>,
    results_scroll: ScrollHandle,
}

impl Workbench {
    pub fn new(
        data: PathBuf,
        preferences: Preferences,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let attacks = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(Vec::<ModeChoice>::new()),
                None,
                window,
                cx,
            )
            .searchable(true)
        });
        cx.subscribe_in(&attacks, window, |this, _, event, window, cx| {
            if let SelectEvent::Confirm(Some(Some(id))) = event {
                this.select_attack(*id, window, cx);
            }
        })
        .detach();
        let mut this = Self {
            page: Page::Task,
            locations: hashcat::ResultLocations::load(&data),
            download: None,
            download_note: None,
            helpers: helpers::QuickSetup::new(window, cx),
            forensics: forensics::Forensics::new(&data, window, cx),
            data,
            executable: None,
            installation: None,
            probing: false,
            engine_notice: None,
            devices: DeviceQuery::Idle,
            picking: false,
            preferences,
            outline: WindowOutline::default(),
            appearance_error: None,
            source: Source::Protected,
            protected_file: None,
            extraction: Extraction::Idle,
            hashes: None,
            hash_text: cx.new(|cx| {
                InputState::new(window, cx)
                    .multi_line(true)
                    .rows(3)
                    .placeholder("每行粘贴一条哈希")
            }),
            modes: cx.new(|cx| {
                SelectState::new(
                    SearchableVec::new(vec![auto_mode()]),
                    Some(IndexPath::default()),
                    window,
                    cx,
                )
                .searchable(true)
            }),
            identifying: false,
            type_notice: None,
            username: false,
            attacks,
            attack: 0,
            operands: cx.new(|cx| {
                InputState::new(window, cx)
                    .multi_line(true)
                    .rows(2)
                    .placeholder("每行填写一个路径或参数")
            }),
            operand_drafts: HashMap::new(),
            words: None,
            second_words: None,
            rule: None,
            mask_file: None,
            mask_text: cx.new(|cx| InputState::new(window, cx).placeholder("例如 ?d?d?d?d")),
            form_error: None,
            tools: EngineTools::new(window, cx),
            cancel: None,
            phase: Phase::Ready,
            status: Status::default(),
            logs: VecDeque::new(),
            log_scroll: ScrollHandle::new(),
            current_result: None,
            found_passwords: Vec::new(),
            run_error: None,
            copied_log: false,
            results: Vec::new(),
            selected_result: None,
            result_preview: None,
            results_scroll: ScrollHandle::new(),
        };
        cx.observe_window_bounds(window, |this, window, cx| {
            this.queue_outline(window, cx);
        })
        .detach();
        this.queue_outline(window, cx);
        if let Some(executable) = hashcat::locate(&this.data) {
            this.connect(executable, false, window, cx);
        }
        this
    }

    fn queue_outline(&self, window: &mut Window, cx: &mut Context<Self>) {
        cx.on_next_frame(window, |this, window, cx| {
            if let Err(error) = this.outline.apply(window, this.preferences.outline) {
                this.appearance_error = Some(format!("无法应用窗口轮廓（{error}）。"));
                cx.notify();
            }
        });
    }

    fn set_appearance(
        &mut self,
        appearance: Appearance,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.preferences.appearance != appearance {
            self.preferences.appearance = appearance;
            configure_theme(appearance, cx);
            self.save_appearance(cx);
            window.refresh();
        }
    }

    fn set_outline(&mut self, outline: Outline, window: &mut Window, cx: &mut Context<Self>) {
        if self.preferences.outline != outline {
            self.preferences.outline = outline;
            self.save_appearance(cx);
            self.queue_outline(window, cx);
            window.refresh();
        }
    }

    fn save_appearance(&mut self, cx: &mut Context<Self>) {
        self.appearance_error = self
            .preferences
            .save(&self.data)
            .err()
            .map(|error| format!("无法保存外观设置（{error}）。"));
        cx.notify();
    }

    fn form_locked(&self) -> bool {
        self.cancel.is_some()
            || self.tools.cancel.is_some()
            || self.identifying
            || matches!(self.extraction, Extraction::Pending)
    }

    fn activate_attack(&mut self, id: u32, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(engine) = &self.installation {
            let items = tools::attack_choices(engine);
            self.attacks.update(cx, |state, cx| {
                state.set_items(items.into(), window, cx);
                state.set_selected_value(&Some(id), window, cx);
            });
        }
        self.select_attack(id, window, cx);
    }

    fn select_attack(&mut self, id: u32, window: &mut Window, cx: &mut Context<Self>) {
        if id != self.attack {
            self.operand_drafts
                .insert(self.attack, self.operands.read(cx).value().to_string());
            self.attack = id;
            let text = self.operand_drafts.remove(&id).unwrap_or_default();
            self.operands
                .update(cx, |state, cx| state.set_value(text, window, cx));
            self.form_error = None;
            cx.notify();
        }
    }

    fn connect(&mut self, path: PathBuf, save: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.executable = Some(path.clone());
        self.installation = None;
        self.engine_notice = None;
        self.type_notice = None;
        self.probing = true;
        self.devices = DeviceQuery::Idle;
        let data = self.data.clone();
        let task = cx.background_spawn(async move {
            let installation = hashcat::probe(&path)?;
            let notice = if save {
                hashcat::save_executable(&data, path)
                    .err()
                    .map(|error| format!("Hashcat 已连接，保存路径失败（{error}）。"))
            } else {
                None
            };
            Ok::<_, String>((installation, notice))
        });
        cx.spawn_in(window, async move |weak, cx| {
            let result = task.await;
            let _ = weak.update_in(cx, |this, window, cx| {
                this.probing = false;
                match result {
                    Ok((installation, notice)) => {
                        this.installation = Some(installation);
                        this.engine_notice = notice.map(|text| Notice { text, error: true });
                    }
                    Err(text) => this.engine_notice = Some(Notice { text, error: true }),
                }
                this.set_modes(&[], window, cx);
                let options = this
                    .installation
                    .as_ref()
                    .map(tools::attack_choices)
                    .unwrap_or_default();
                let index = options
                    .iter()
                    .position(|entry| entry.id == Some(this.attack))
                    .or_else(|| (!options.is_empty()).then_some(0));
                if let Some(id) = index.and_then(|i| options[i].id) {
                    this.select_attack(id, window, cx);
                }
                this.attacks.update(cx, |select, cx| {
                    select.set_items(SearchableVec::new(options), window, cx);
                    select.set_selected_index(
                        index.map(|i| IndexPath::default().row(i)),
                        window,
                        cx,
                    );
                });
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn set_modes(&mut self, matches: &[Mode], window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.modes.read(cx).selected_value().copied().flatten();
        let mut options = vec![auto_mode()];
        options.extend(matches.iter().map(ModeChoice::from));
        if let Some(installation) = &self.installation {
            options.extend(
                installation
                    .modes
                    .iter()
                    .filter(|mode| !matches.iter().any(|matched| matched.id == mode.id))
                    .map(ModeChoice::from),
            );
        }
        let selected = if matches.len() == 1 {
            Some(matches[0].id)
        } else {
            selected
        };
        let index = options
            .iter()
            .position(|mode| mode.id == selected)
            .unwrap_or(0);
        self.modes.update(cx, |state, cx| {
            state.set_items(SearchableVec::new(options), window, cx);
            state.set_selected_index(Some(IndexPath::default().row(index)), window, cx);
        });
    }

    fn pick_file(&mut self, slot: FileSlot, window: &mut Window, cx: &mut Context<Self>) {
        if self.form_locked() || self.picking {
            return;
        }
        self.picking = true;
        let directory = matches!(
            slot,
            FileSlot::OperandDirectory | FileSlot::ArgumentDirectory | FileSlot::ResultsDirectory
        );
        let multiple = matches!(slot, FileSlot::OperandFile | FileSlot::ArgumentFile);
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: !directory,
            directories: directory,
            multiple,
            prompt: Some(slot.prompt().into()),
        });
        cx.spawn_in(window, async move |weak, cx| {
            let result = picker.await;
            let _ = weak.update_in(cx, move |this, window, cx| {
                this.picking = false;
                match result {
                    Ok(Ok(Some(paths))) => {
                        for path in paths {
                            this.set_file(slot, path, window, cx);
                        }
                    }
                    Ok(Ok(None)) => {}
                    _ if matches!(
                        slot,
                        FileSlot::ArgumentFile | FileSlot::ArgumentDirectory | FileSlot::Stdin
                    ) =>
                    {
                        this.tools.status = "无法打开路径选择器。".into();
                    }
                    _ if matches!(slot, FileSlot::Executable | FileSlot::ResultsDirectory) => {
                        this.engine_notice = Some(Notice {
                            text: "无法打开文件选择器。".into(),
                            error: true,
                        });
                    }
                    _ => this.form_error = Some("无法打开文件选择器。".into()),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn set_file(
        &mut self,
        slot: FileSlot,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.form_error = None;
        match slot {
            FileSlot::Protected => {
                self.protected_file = Some(path.clone());
                self.extraction = Extraction::Pending;
                let task = cx.background_spawn(async move { protected::extract(&path) });
                cx.spawn_in(window, async move |weak, cx| {
                    let result = task.await;
                    let _ = weak.update_in(cx, |this, _, cx| {
                        this.extraction = match result {
                            Ok(data) => Extraction::Ready(data),
                            Err(error) => Extraction::Failed(error),
                        };
                        cx.notify();
                    });
                })
                .detach();
            }
            FileSlot::Hashes => {
                self.hashes = Some(path);
                self.type_notice = None;
            }
            FileSlot::Words => self.words = Some(path),
            FileSlot::SecondWords => self.second_words = Some(path),
            FileSlot::Rule => self.rule = Some(path),
            FileSlot::Mask => self.mask_file = Some(path),
            FileSlot::OperandFile | FileSlot::OperandDirectory => {
                append_lines(&self.operands, &path.to_string_lossy(), window, cx)
            }
            FileSlot::ArgumentFile | FileSlot::ArgumentDirectory => {
                append_lines(&self.tools.arguments, &path.to_string_lossy(), window, cx)
            }
            FileSlot::Stdin => self.tools.stdin = Some(path),
            FileSlot::ResultsDirectory => self.change_results_directory(path, cx),
            FileSlot::Executable => {
                if path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
                {
                    self.connect(path, true, window, cx);
                } else {
                    self.engine_notice = Some(Notice {
                        text: "请选择 Hashcat 的 .exe 文件。".into(),
                        error: true,
                    });
                }
            }
        }
        cx.notify();
    }

    fn hash_input(&self, cx: &App) -> Result<HashInput, String> {
        if self.source == Source::Paste {
            let text = self.hash_text.read(cx).value();
            if text.trim().is_empty() {
                return Err("请粘贴至少一条哈希。".into());
            }
            Ok(HashInput::Pasted(text.to_string()))
        } else {
            required_file(self.hashes.as_deref(), "哈希文件").map(HashInput::File)
        }
    }

    fn identify(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.form_locked() || self.picking || self.installation.is_none() {
            return;
        }
        let Some(path) = self.executable.clone() else {
            return;
        };
        let input = match self.hash_input(cx) {
            Ok(input) => input,
            Err(text) => {
                self.type_notice = Some(Notice { text, error: true });
                cx.notify();
                return;
            }
        };
        self.identifying = true;
        self.type_notice = None;
        let username = self.username;
        let task = cx.background_spawn(async move { hashcat::identify(&path, input, username) });
        cx.spawn_in(window, async move |weak, cx| {
            let result = task.await;
            let _ = weak.update_in(cx, |this, window, cx| {
                this.identifying = false;
                this.type_notice = Some(match result {
                    Ok(modes) if !modes.is_empty() => {
                        let text = if modes.len() == 1 {
                            format!("已选中 {}。", modes[0].name)
                        } else {
                            format!(
                                "匹配 {} 种格式，已置顶候选。请选择实际使用的算法。",
                                modes.len()
                            )
                        };
                        this.set_modes(&modes, window, cx);
                        Notice { text, error: false }
                    }
                    Ok(_) => Notice {
                        text: "没有找到匹配的哈希类型。".into(),
                        error: true,
                    },
                    Err(text) => Notice { text, error: true },
                });
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn request(&self, cx: &App) -> Result<Request, String> {
        let password_only = self.source == Source::Protected;
        let (input, mode, source_name) = if password_only {
            let path = required_file(self.protected_file.as_deref(), "加密文件")?;
            let Extraction::Ready(data) = &self.extraction else {
                return Err("请选择可提取校验数据的加密文件。".into());
            };
            if self
                .installation
                .as_ref()
                .is_some_and(|engine| !engine.modes.iter().any(|mode| mode.id == data.mode))
            {
                return Err(format!("当前 Hashcat 安装缺少模式 {}。", data.mode));
            }
            (
                HashInput::Pasted(data.hash.clone()),
                Some(data.mode),
                hashcat::file_name(&path),
            )
        } else {
            let input = self.hash_input(cx)?;
            let mode = self.modes.read(cx).selected_value().copied().flatten();
            let label = match &input {
                HashInput::File(path) => hashcat::file_name(path),
                HashInput::Pasted(_) => "粘贴的哈希".into(),
            };
            (input, mode, label)
        };
        let words = || required_file(self.words.as_deref(), "字典文件");
        let mask = || {
            if self.mask_file.is_some() {
                return required_file(self.mask_file.as_deref(), "掩码文件")
                    .map(PathBuf::into_os_string);
            }
            let text = self.mask_text.read(cx).value();
            if text.is_empty() {
                Err("请输入掩码或选择掩码文件。".into())
            } else {
                Ok(OsString::from(text.as_ref()))
            }
        };
        let rule = || {
            self.rule
                .as_deref()
                .map(|path| required_file(Some(path), "规则文件"))
                .transpose()
        };
        let candidates = match self.attack {
            0 => Candidates::Dictionary {
                words: words()?,
                rule: rule()?,
            },
            3 => Candidates::Mask(mask()?),
            1 => Candidates::Combination {
                left: words()?,
                right: required_file(self.second_words.as_deref(), "第二份字典")?,
            },
            6 | 7 => Candidates::Hybrid {
                words: words()?,
                mask: mask()?,
                mask_first: self.attack == 7,
            },
            attack => {
                let mut inputs = hashcat::argument_lines(&self.operands.read(cx).value());
                match attack {
                    4 if inputs.len() > 1 => return Err("PCFG 最多接受一个语法包或目录。".into()),
                    5 if inputs.is_empty() => return Err("请添加至少一个变换表。".into()),
                    5 => inputs.insert(0, words()?.into_os_string()),
                    8 if inputs.is_empty() => return Err("请在第一行指定候选源模块。".into()),
                    9 if inputs.len() > 1 => return Err("逐条关联最多接受一个候选字典。".into()),
                    12 if inputs.len() > 1 => return Err("请仅添加与 ?q 对应的第二份字典。".into()),
                    12 => {
                        inputs.insert(0, words()?.into_os_string());
                        inputs.insert(0, mask()?);
                    }
                    _ => {}
                }
                Candidates::Positional {
                    attack,
                    operands: inputs,
                    rule: if attack == 9 { rule()? } else { None },
                }
            }
        };
        Ok(Request {
            input,
            mode,
            candidates,
            username: self.username && !password_only,
            source_name,
            password_only,
        })
    }

    fn start_task(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.form_locked() || self.picking || self.probing {
            return;
        }
        let Some(executable) = self
            .executable
            .clone()
            .filter(|_| self.installation.is_some())
        else {
            self.change_page(Page::Settings, cx);
            return;
        };
        let request = match self.request(cx) {
            Ok(request) => request,
            Err(error) => {
                self.form_error = Some(error);
                cx.notify();
                return;
            }
        };
        self.logs.clear();
        self.copied_log = false;
        self.status = Status::default();
        self.form_error = None;
        self.run_error = None;
        self.current_result = None;
        self.found_passwords.clear();
        self.phase = Phase::Starting;
        let (cancel, events) = hashcat::start(executable, self.data.clone(), request);
        self.cancel = Some(cancel);
        cx.spawn_in(window, async move |weak, cx| {
            while let Ok(first) = events.recv().await {
                let result = weak.update_in(cx, |this, _, cx| {
                    let more = std::iter::from_fn(|| events.try_recv().ok()).take(100);
                    for event in std::iter::once(first).chain(more) {
                        this.on_run_event(event);
                    }
                    if this.page == Page::Results && this.cancel.is_none() {
                        this.refresh_results();
                    }
                    cx.notify();
                });
                if result.is_err() {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    fn on_run_event(&mut self, event: RunEvent) {
        match event {
            RunEvent::ResultPath(path) => self.current_result = Some(path),
            RunEvent::Passwords(passwords) => self.found_passwords.extend(passwords),
            RunEvent::Status(status) => {
                if self.phase == Phase::Starting {
                    self.phase = Phase::Running;
                }
                self.status = status;
            }
            RunEvent::Log(line) => {
                if self.logs.len() == LOG_LIMIT {
                    self.logs.pop_front();
                }
                let follow =
                    self.log_scroll.offset().y + self.log_scroll.max_offset().height <= px(24.);
                self.logs.push_back(line.into());
                if follow {
                    self.log_scroll.scroll_to_bottom();
                }
                self.copied_log = false;
            }
            RunEvent::Finished(outcome) => {
                self.cancel = None;
                self.phase = match outcome {
                    Ok(Outcome::Recovered) => Phase::Recovered,
                    Ok(Outcome::Exhausted) => Phase::Exhausted,
                    Ok(Outcome::Stopped) => Phase::Stopped,
                    Err(error) => {
                        self.run_error = Some(error);
                        Phase::Failed
                    }
                };
            }
        }
    }

    fn stop_task(&mut self, cx: &mut Context<Self>) {
        if let Some(cancel) = &self.cancel {
            cancel.stop();
            self.phase = Phase::Stopping;
            cx.notify();
        }
    }

    fn refresh_devices(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.installation.is_none()
            || self.form_locked()
            || matches!(self.devices, DeviceQuery::Pending)
        {
            return;
        }
        let Some(executable) = self.executable.clone() else {
            return;
        };
        self.devices = DeviceQuery::Pending;
        let task = cx.background_spawn(async move { hashcat::devices(&executable) });
        cx.spawn_in(window, async move |weak, cx| {
            let output = task.await;
            let _ = weak.update_in(cx, |this, _, cx| {
                this.devices = match output {
                    Ok(text) => DeviceQuery::Ready(text),
                    Err(text) => DeviceQuery::Failed(text),
                };
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn change_page(&mut self, page: Page, cx: &mut Context<Self>) {
        self.page = page;
        if page == Page::Results {
            self.refresh_results();
        }
        cx.notify();
    }

    fn refresh_results(&mut self) {
        self.results = hashcat::results(&self.data);
        let path = self
            .selected_result
            .as_ref()
            .filter(|path| self.results.iter().any(|file| &file.path == *path))
            .cloned()
            .or_else(|| self.results.first().map(|file| file.path.clone()));
        self.select_result(path);
    }

    fn select_result(&mut self, path: Option<PathBuf>) {
        self.result_preview = path.as_ref().map(|path| {
            hashcat::preview(path).map_err(|error| format!("无法读取结果文件（{error}）。"))
        });
        if self.selected_result != path {
            self.results_scroll.set_offset(gpui::point(px(0.), px(0.)));
        }
        self.selected_result = path;
    }

    fn open_data_dir(&mut self, cx: &mut Context<Self>) {
        let folder = self.locations.active(&self.data);
        match fs::create_dir_all(&folder) {
            Ok(()) => cx.reveal_path(&folder),
            Err(error) => {
                self.engine_notice = Some(Notice {
                    text: format!("无法打开数据目录（{error}）。"),
                    error: true,
                })
            }
        }
        cx.notify();
    }
}

fn append_lines(
    input: &Entity<InputState>,
    addition: &str,
    window: &mut Window,
    cx: &mut Context<Workbench>,
) {
    input.update(cx, |state, cx| {
        let mut text = state.value().to_string();
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(addition);
        state.set_value(text, window, cx);
    });
}

fn required_file(path: Option<&Path>, label: &str) -> Result<PathBuf, String> {
    let path = path.ok_or_else(|| format!("请选择{label}。"))?;
    if !path.is_absolute() || !path.is_file() {
        return Err(format!("{label}已不存在，请重新选择。"));
    }
    Ok(path.to_path_buf())
}
