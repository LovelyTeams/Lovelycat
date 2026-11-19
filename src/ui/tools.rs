use std::{collections::VecDeque, ops::Range, path::PathBuf};

use gpui::{
    AppContext, ClipboardItem, Context, Div, Entity, InteractiveElement, IntoElement,
    ParentElement, ScrollHandle, ScrollStrategy, SharedString, StatefulInteractiveElement, Styled,
    UniformListScrollHandle, Window, div, prelude::FluentBuilder, px, relative, rgb, uniform_list,
};
use gpui_component::{
    Disableable, Icon, IconName,
    input::{InputEvent, InputState},
};

use super::layout::{button, choice, multiline, panel, primary, section_title, single_line};
use super::palette::colors;
use super::{FileSlot, LOG_LIMIT, Page, Source, Workbench, append_lines};
use crate::hashcat::{self, CommandEvent, Installation};
use crate::process::Cancel;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Modes,
    Attacks,
    Options,
}

pub(super) struct EngineTools {
    section: Section,
    search: Entity<InputState>,
    list: UniformListScrollHandle,
    pub(super) arguments: Entity<InputState>,
    pub(super) stdin: Option<PathBuf>,
    pub(super) cancel: Option<Cancel>,
    pub(super) status: String,
    logs: VecDeque<SharedString>,
    scroll: ScrollHandle,
    copied_output: bool,
}

impl EngineTools {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Workbench>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("搜索编号、名称或关键词"));
        cx.subscribe(&search, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.tools
                    .list
                    .scroll_to_item_strict(0, ScrollStrategy::Top);
                cx.notify();
            }
        })
        .detach();
        Self {
            section: Section::Modes,
            search,
            list: UniformListScrollHandle::new(),
            arguments: cx.new(|cx| {
                InputState::new(window, cx)
                    .multi_line(true)
                    .rows(4)
                    .placeholder("例如 --version")
            }),
            stdin: None,
            cancel: None,
            status: "就绪".into(),
            logs: VecDeque::new(),
            scroll: ScrollHandle::new(),
            copied_output: false,
        }
    }
}

impl Section {
    fn matches(self, engine: &Installation, query: &str) -> Vec<usize> {
        let words: Vec<_> = query.split_whitespace().map(str::to_lowercase).collect();
        let matches = |fields: &[&str]| {
            words.is_empty() || {
                let text = fields.join(" ").to_lowercase();
                words.iter().all(|word| text.contains(word))
            }
        };
        match self {
            Self::Modes => engine
                .modes
                .iter()
                .enumerate()
                .filter(|(_, m)| matches(&[&m.id.to_string(), &m.name, &m.category]))
                .map(|(i, _)| i)
                .collect(),
            Self::Attacks => engine
                .attacks
                .iter()
                .enumerate()
                .filter(|(_, a)| matches(&[&a.id.to_string(), a.label(), &a.name]))
                .map(|(i, _)| i)
                .collect(),
            Self::Options => engine
                .options
                .iter()
                .enumerate()
                .filter(|(_, o)| matches(&[&o.flags, &o.kind, &o.description]))
                .map(|(i, _)| i)
                .collect(),
        }
    }
}

impl Workbench {
    pub(super) fn tools_page(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let compact = window.viewport_size().width < px(940.);
        div()
            .id("engine-tools-page")
            .flex()
            .flex_1()
            .min_h_0()
            .gap(px(16.))
            .px(px(24.))
            .pb(px(20.))
            .when(compact, |this| this.flex_col().overflow_y_scroll())
            .child(self.catalog(compact, cx))
            .child(self.command_panel(compact, cx))
    }

    fn catalog(&self, compact: bool, cx: &mut Context<Self>) -> Div {
        let c = colors(cx);
        let section = self.tools.section;
        let engine = self.installation.as_ref();
        let indices = engine
            .map(|e| section.matches(e, &self.tools.search.read(cx).value()))
            .unwrap_or_default();
        let count = indices.len();
        let tabs = [
            ("types", "哈希类型", Section::Modes),
            ("methods", "候选模式", Section::Attacks),
            ("flags", "命令选项", Section::Options),
        ];
        let header = div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .p(px(18.))
            .flex_shrink_0()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(section_title("功能目录"))
                    .child(
                        div()
                            .px(px(8.))
                            .py(px(2.))
                            .rounded_full()
                            .bg(rgb(c.raised))
                            .text_size(px(12.))
                            .text_color(rgb(c.muted))
                            .child(format!("{count} 项")),
                    ),
            )
            .child(
                div()
                    .flex()
                    .p(px(3.))
                    .rounded(px(9.))
                    .bg(rgb(c.background))
                    .border_1()
                    .border_color(rgb(c.subtle_border))
                    .gap(px(3.))
                    .children(tabs.into_iter().map(|(id, label, tab)| {
                        choice(id, label, section == tab, cx)
                            .flex_1()
                            .h(px(30.))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if this.tools.section != tab {
                                    this.tools.section = tab;
                                    this.tools
                                        .search
                                        .update(cx, |state, cx| state.set_value("", window, cx));
                                    this.tools
                                        .list
                                        .scroll_to_item_strict(0, ScrollStrategy::Top);
                                    cx.notify();
                                }
                            }))
                    })),
            )
            .child(
                single_line(&self.tools.search)
                    .prefix(
                        Icon::new(IconName::Search)
                            .size(px(16.))
                            .text_color(rgb(c.muted)),
                    )
                    .cleanable(true),
            );
        panel(c)
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .when(compact, |this| this.h(px(340.)).w_full().flex_shrink_0())
            .when(!compact, |this| this.w(relative(0.44)))
            .child(header)
            .child(if count == 0 {
                div()
                    .p(px(16.))
                    .text_color(rgb(c.muted))
                    .child(if self.probing {
                        "正在读取 Hashcat…"
                    } else if engine.is_none() {
                        "连接 Hashcat 后可浏览功能目录"
                    } else {
                        "没有匹配项"
                    })
                    .into_any_element()
            } else {
                uniform_list(
                    "engine-catalog",
                    count,
                    cx.processor(move |this, range: Range<usize>, _, cx| {
                        range
                            .filter_map(|i| this.catalog_row(section, *indices.get(i)?, cx))
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(self.tools.list.clone())
                .flex_1()
                .min_h_0()
                .into_any_element()
            })
    }

    fn catalog_row(&self, section: Section, index: usize, cx: &mut Context<Self>) -> Option<Div> {
        let c = colors(cx);
        let engine = self.installation.as_ref()?;
        let (title, details, arguments, id) = match section {
            Section::Modes => {
                let m = engine.modes.get(index)?;
                (
                    format!("{}  ·  {}", m.id, m.name),
                    m.category.clone(),
                    format!("-m\n{}", m.id),
                    Some(m.id),
                )
            }
            Section::Attacks => {
                let a = engine.attacks.get(index)?;
                (
                    format!("-a {}  ·  {}", a.id, a.label()),
                    a.name.clone(),
                    format!("-a\n{}", a.id),
                    Some(a.id),
                )
            }
            Section::Options => {
                let o = engine.options.get(index)?;
                let title = if o.kind.is_empty() {
                    o.flags.clone()
                } else {
                    format!("{}  ‹{}›", o.flags, o.kind)
                };
                (title, o.description.clone(), o.flag.clone(), None)
            }
        };
        let hint = format!("{title}\n{details}");
        let locked = self.form_locked() || self.picking;
        let row = div()
            .flex()
            .items_center()
            .h(px(56.))
            .px(px(18.))
            .gap(px(8.))
            .border_t_1()
            .border_color(rgb(c.subtle_border))
            .hover(|style| style.bg(rgb(c.hover)))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_ellipsis()
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .child(title),
                    )
                    .child(
                        div()
                            .text_ellipsis()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(rgb(c.muted))
                            .child(details),
                    ),
            )
            .when_some(id, |row, id| {
                row.child(
                    button(("use-in-task", index), cx)
                        .h(px(28.))
                        .px(px(9.))
                        .label("用于任务")
                        .disabled(locked)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if section == Section::Modes {
                                if this.source == Source::Protected {
                                    this.source = if this.hashes.is_some() {
                                        Source::HashFile
                                    } else {
                                        Source::Paste
                                    };
                                }
                                this.type_notice = None;
                                // Reset any previous search filter before selecting a catalog item.
                                this.set_modes(&[], window, cx);
                                this.modes.update(cx, |state, cx| {
                                    state.set_selected_value(&Some(id), window, cx)
                                });
                            } else {
                                this.activate_attack(id, window, cx);
                            }
                            this.form_error = None;
                            this.change_page(Page::Task, cx);
                        })),
                )
            })
            .child(
                button(("append-argument", index), cx)
                    .w(px(30.))
                    .h(px(28.))
                    .p_0()
                    .child(Icon::new(IconName::Plus).size(px(14.)))
                    .tooltip(format!("添加到参数\n{hint}"))
                    .disabled(locked)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        append_lines(&this.tools.arguments, &arguments, window, cx);
                        this.tools.status =
                            format!("已添加参数：{}", arguments.replace('\n', " "));
                        cx.notify();
                    })),
            );
        Some(row)
    }

    fn command_panel(&self, compact: bool, cx: &mut Context<Self>) -> Div {
        let c = colors(cx);
        let locked =
            self.form_locked() || self.probing || self.picking || self.installation.is_none();
        let busy = self.tools.cancel.is_some();
        let presets = [
            ("preset-version", "版本", "--version"),
            ("preset-device", "设备", "-I"),
            ("preset-bench", "基准测试", "--benchmark"),
        ];
        let has_args = !self.tools.arguments.read(cx).value().trim().is_empty();
        let editor = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .p(px(18.))
            .flex_shrink_0()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(section_title("本机命令"))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .children(presets.into_iter().map(|(id, label, preset)| {
                                button(id, cx)
                                    .h(px(28.))
                                    .px(px(8.))
                                    .label(label)
                                    .tooltip(format!("填入 {preset}"))
                                    .disabled(locked)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.tools.arguments.update(cx, |state, cx| {
                                            state.set_value(preset, window, cx)
                                        });
                                    }))
                            }))
                            .when(has_args, |row| {
                                row.child(
                                    button("clear-args", cx)
                                        .h(px(28.))
                                        .px(px(8.))
                                        .label("清空")
                                        .tooltip("清空所有参数")
                                        .disabled(locked)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.tools
                                                .arguments
                                                .update(cx, |state, cx| state.set_value("", window, cx));
                                            this.tools.stdin = None;
                                            cx.notify();
                                        })),
                                )
                            }),
                    ),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(rgb(c.muted))
                    .child("每行一个参数。含空格的路径直接填写，无需加引号。按 Ctrl+Enter 快速运行。"),
            )
            .child(
                multiline(&self.tools.arguments, 108.)
                    .font_family("Cascadia Mono")
                    .disabled(busy),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(6.))
                    .children(
                        [
                            ("add-arg-file", "添加文件", FileSlot::ArgumentFile),
                            ("add-arg-dir", "添加目录", FileSlot::ArgumentDirectory),
                            ("add-stdin", "标准输入文件", FileSlot::Stdin),
                        ]
                        .into_iter()
                        .map(|(id, label, slot)| {
                            button(id, cx)
                                .h(px(30.))
                                .px(px(9.))
                                .label(label)
                                .disabled(locked)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.pick_file(slot, window, cx)
                                }))
                        }),
                    )
                    .when_some(self.tools.stdin.as_ref(), |row, path| {
                        row.child(
                            button("clear-stdin", cx)
                                .h(px(30.))
                                .px(px(9.))
                                .label(format!("stdin: {} ×", hashcat::file_name(path)))
                                .tooltip(format!("清除标准输入文件 {}", path.display()))
                                .disabled(locked)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.tools.stdin = None;
                                    cx.notify();
                                })),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .items_center()
                    .map(|row| {
                        if busy {
                            row.child(
                                button("stop-command", cx)
                                    .h(px(36.))
                                    .px(px(14.))
                                    .label("停止命令")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(cancel) = &this.tools.cancel {
                                            cancel.stop();
                                            this.tools.status = "正在停止".into();
                                            cx.notify();
                                        }
                                    })),
                            )
                        } else {
                            row.child(
                                primary("run-command", cx)
                                    .h(px(36.))
                                    .px(px(14.))
                                    .label("运行命令")
                                    .tooltip("运行当前命令 (Ctrl+Enter)")
                                    .disabled(locked)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.run_command(false, None, window, cx)
                                    })),
                            )
                            .child(
                                button("export-stdout", cx)
                                    .h(px(36.))
                                    .px(px(12.))
                                    .label("运行并导出")
                                    .tooltip("将完整标准输出直接写入所选文件")
                                    .disabled(locked)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.export_command(window, cx)
                                    })),
                            )
                            .when(cfg!(windows), |row| {
                                row.child(
                                    button("terminal-command", cx)
                                        .h(px(36.))
                                        .px(px(12.))
                                        .label("交互终端")
                                        .tooltip("在独立终端中手动操作 Hashcat")
                                        .disabled(locked || self.tools.stdin.is_some())
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.run_command(true, None, window, cx)
                                        })),
                                )
                            })
                        }
                    }),
            );
        let output = div()
            .flex()
            .flex_col()
            .min_h_0()
            .flex_1()
            .border_t_1()
            .border_color(rgb(c.border))
            .bg(rgb(c.monitor))
            .child(
                div()
                    .h(px(44.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(18.))
                    .border_b_1()
                    .border_color(rgb(c.subtle_border))
                    .child(section_title("输出"))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .text_ellipsis()
                            .text_size(px(12.))
                            .text_color(rgb(c.muted))
                            .child(self.tools.status.clone()),
                    )
                    .when(!self.tools.logs.is_empty() && !busy, |row| {
                        row.child(
                            button("clear-command-output", cx)
                                .h(px(28.))
                                .px(px(8.))
                                .label("清空")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.tools.logs.clear();
                                    this.tools.copied_output = false;
                                    this.tools.status = "就绪".into();
                                    cx.notify();
                                })),
                        )
                    })
                    .child(
                        button("copy-command-output", cx)
                            .h(px(28.))
                            .px(px(9.))
                            .gap(px(5.))
                            .child(
                                Icon::new(if self.tools.copied_output {
                                    IconName::Check
                                } else {
                                    IconName::Copy
                                })
                                .size(px(13.))
                                .text_color(rgb(if self.tools.copied_output {
                                    c.success
                                } else {
                                    c.muted
                                })),
                            )
                            .child(if self.tools.copied_output {
                                "已复制"
                            } else {
                                "复制"
                            })
                            .disabled(self.tools.logs.is_empty())
                            .on_click(cx.listener(|this, _, _, cx| {
                                let text = this
                                    .tools
                                    .logs
                                    .iter()
                                    .map(|line| line.as_ref())
                                    .collect::<Vec<_>>()
                                    .join("\n");
                                cx.write_to_clipboard(ClipboardItem::new_string(text));
                                this.tools.copied_output = true;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .id("command-output")
                    .flex_1()
                    .min_h_0()
                    .overflow_scroll()
                    .track_scroll(&self.tools.scroll)
                    .px(px(18.))
                    .py(px(12.))
                    .font_family("Cascadia Mono")
                    .text_size(px(13.))
                    .line_height(px(20.))
                    .text_color(rgb(c.muted))
                    .when(self.tools.logs.is_empty(), |this| {
                        this.child("命令输出将显示在这里")
                    })
                    .children(
                        self.tools
                            .logs
                            .iter()
                            .cloned()
                            .map(|line| div().whitespace_nowrap().child(line)),
                    ),
            );
        panel(c)
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .when(compact, |this| this.h(px(548.)).flex_shrink_0())
            .when(!compact, |this| this.flex_1())
            .child(editor)
            .child(output)
    }

    fn export_command(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.form_locked() || self.picking {
            return;
        }
        if hashcat::argument_lines(&self.tools.arguments.read(cx).value()).is_empty() {
            self.tools.status = "请先填写 Hashcat 参数。".into();
            cx.notify();
            return;
        }
        let directory = std::env::current_dir().unwrap_or_else(|_| self.data.clone());
        let picker = cx.prompt_for_new_path(&directory, Some("hashcat-output.txt"));
        self.picking = true;
        cx.spawn_in(window, async move |weak, cx| {
            let result = picker.await;
            let _ = weak.update_in(cx, |this, window, cx| {
                this.picking = false;
                match result {
                    Ok(Ok(Some(path))) => this.run_command(false, Some(path), window, cx),
                    Ok(Ok(None)) => {}
                    _ => this.tools.status = "无法打开保存对话框。".into(),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn run_command(
        &mut self,
        terminal: bool,
        output: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.form_locked() || self.picking || self.installation.is_none() {
            return;
        }
        let Some(exe) = self.executable.clone() else {
            return;
        };
        let args = hashcat::argument_lines(&self.tools.arguments.read(cx).value());
        if args.is_empty() {
            self.tools.status = "请先填写 Hashcat 参数。".into();
            cx.notify();
            return;
        }
        let saved = output.clone();
        let (cancel, events) =
            hashcat::start_command(exe, args, terminal, self.tools.stdin.clone(), output);
        self.tools.cancel = Some(cancel);
        self.tools.logs.clear();
        self.tools.copied_output = false;
        self.tools.scroll.set_offset(gpui::point(px(0.), px(0.)));
        self.tools.status = if terminal {
            "在独立终端中运行"
        } else {
            "运行中"
        }
        .into();
        cx.spawn_in(window, async move |weak, cx| {
            while let Ok(event) = events.recv().await {
                if weak
                    .update_in(cx, |this, _, cx| {
                        let follow = this.tools.scroll.offset().y
                            + this.tools.scroll.max_offset().height
                            <= px(24.);
                        for event in std::iter::once(event)
                            .chain(std::iter::from_fn(|| events.try_recv().ok()).take(128))
                        {
                            match event {
                                CommandEvent::Line(line) => {
                                    if this.tools.logs.len() == LOG_LIMIT {
                                        this.tools.logs.pop_front();
                                    }
                                    this.tools.logs.push_back(line.into());
                                }
                                CommandEvent::Finished(result) => {
                                    this.tools.cancel = None;
                                    this.tools.status = match result {
                                        Ok(code) => {
                                            let state = code.map_or_else(
                                                || "已停止".into(),
                                                |code| format!("退出码 {code}"),
                                            );
                                            if let Some(path) = &saved {
                                                format!("{state} · 已导出至 {}", path.display())
                                            } else {
                                                state
                                            }
                                        }
                                        Err(error) => error,
                                    };
                                }
                            }
                        }
                        if follow {
                            this.tools.scroll.scroll_to_bottom();
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }
}

pub(super) fn attack_choices(engine: &Installation) -> Vec<super::ModeChoice> {
    engine
        .attacks
        .iter()
        .map(|a| super::ModeChoice {
            id: Some(a.id),
            label: format!("{}  ·  -a {}", a.label(), a.id).into(),
        })
        .collect()
}
