use std::sync::{Arc, LazyLock};

use chrono::{DateTime, Local};
use gpui::{
    AnyElement, App, ClipboardItem, Context, Div, ElementId, Entity, ExternalPaths, FontWeight,
    InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Window, WindowControlArea, canvas, div, point,
    prelude::FluentBuilder, px, relative, rgb,
};
use gpui_component::{
    Disableable, Icon, IconName, Sizable,
    button::{Button, ButtonCustomVariant, ButtonVariants},
    input::{Input, InputState},
    select::Select,
};

use super::{DeviceQuery, Extraction, FileSlot, Notice, Page, Phase, Source, Workbench, hashcat};

use super::palette::{Palette, colors};
use crate::desktop::{Appearance, CAT_EAR_HEIGHT, Outline};

impl Render for Workbench {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = colors(cx);
        let cat = cfg!(windows)
            && self.preferences.outline == Outline::Cat
            && !window.is_maximized()
            && !window.is_fullscreen();
        div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .when(!cat, |root| root.bg(rgb(colors.background)))
            .when(cat, |root| root.child(cat_surface(colors)))
            .text_color(rgb(colors.ink))
            .text_size(px(14.))
            .line_height(px(20.))
            .when(cat, |this| this.child(cat_ears(colors)))
            .child(self.toolbar(window, cat, cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .max_w(px(1520.))
                    .mx_auto()
                    .child(self.page_heading(colors))
                    .child(match self.page {
                        Page::Task => self.task_page(window, cx).into_any_element(),
                        Page::Tools => self.tools_page(window, cx).into_any_element(),
                        Page::Results => self.results_page(cx).into_any_element(),
                        Page::Forensics => self.forensics_page(window, cx).into_any_element(),
                        Page::Settings => self.environment_page(cx).into_any_element(),
                    }),
            )
    }
}

impl Workbench {
    fn toolbar(&self, window: &Window, cat: bool, cx: &mut Context<Self>) -> Div {
        let colors = colors(cx);
        let compact = window.viewport_size().width < px(1040.);
        let narrow = window.viewport_size().width < px(800.);
        let (engine, engine_color) = if self.probing {
            ("正在检测 Hashcat".to_owned(), colors.muted)
        } else if let Some(installation) = &self.installation {
            (format!("Hashcat {}", installation.version), colors.success)
        } else {
            ("连接 Hashcat".to_owned(), colors.error)
        };
        let nav = [
            (
                Page::Task,
                "nav-task",
                "恢复任务",
                IconName::LayoutDashboard,
            ),
            (
                Page::Results,
                "nav-results",
                "恢复结果",
                IconName::FolderOpen,
            ),
            (
                Page::Tools,
                "nav-tools",
                "引擎工具",
                IconName::SquareTerminal,
            ),
            (
                Page::Forensics,
                "nav-forensics",
                "磁盘取证",
                IconName::Inspector,
            ),
            (Page::Settings, "nav-engine", "设置", IconName::Settings2),
        ];
        let next_mode = self.preferences.appearance.toggle();
        let (theme_icon, theme_label) = match next_mode {
            Appearance::Dark | Appearance::Midnight => (IconName::Moon, "切换到深色"),
            _ => (IconName::Sun, "切换到浅色"),
        };
        div()
            .flex()
            .items_center()
            .h(px(56.))
            .flex_shrink_0()
            .px(px(if compact { 12. } else { 24. }))
            .gap(px(if narrow {
                5.
            } else if compact {
                8.
            } else {
                16.
            }))
            .when(!cat, |bar| bar.bg(rgb(colors.panel)))
            .border_b_1()
            .border_color(rgb(colors.border))
            .child(
                div()
                    .id("brand")
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h_full()
                    .flex_shrink_0()
                    .window_control_area(WindowControlArea::Drag)
                    .child(logo())
                    .when(!compact, |this| {
                        this.child(
                            div()
                                .text_size(px(20.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .line_height(px(24.))
                                .text_color(rgb(colors.ink))
                                .child("Lovelycat"),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .children(nav.into_iter().map(|(page, id, title, glyph)| {
                        let selected = self.page == page;
                        let fill = if selected {
                            colors.raised
                        } else {
                            colors.panel
                        };
                        let foreground = if selected { colors.ink } else { colors.muted };
                        button(id, cx)
                            .custom(variant(cx, fill, foreground, fill, colors.raised))
                            .h(px(36.))
                            .px(px(if narrow {
                                5.
                            } else if compact {
                                8.
                            } else {
                                10.
                            }))
                            .child(icon(glyph, foreground, 16.))
                            .child(
                                div()
                                    .text_size(px(if narrow { 13. } else { 14. }))
                                    .text_color(rgb(foreground))
                                    .child(title),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| this.change_page(page, cx)))
                    })),
            )
            .child(
                div()
                    .id("drag-fill")
                    .flex_1()
                    .min_w(px(12.))
                    .h_full()
                    .window_control_area(WindowControlArea::Drag),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .flex_shrink_0()
                    .when(!compact, |this| {
                        this.child(
                            button("engine-status", cx)
                                .h(px(36.))
                                .px(px(12.))
                                .child(dot(engine_color))
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .text_color(rgb(colors.ink))
                                        .child(engine),
                                )
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.change_page(Page::Settings, cx)
                                })),
                        )
                    })
                    .child(
                        toolbar_icon("data-directory", IconName::Folder, "打开结果目录", cx)
                            .on_click(cx.listener(|this, _, _, cx| this.open_data_dir(cx))),
                    )
                    .child(
                        toolbar_icon("toggle-theme", theme_icon, theme_label, cx).on_click(
                            cx.listener(move |this, _, window, cx| {
                                this.set_appearance(next_mode, window, cx)
                            }),
                        ),
                    ),
            )
            .when(cfg!(windows), |this| {
                this.child(self.window_controls(window, cx))
            })
    }

    fn window_controls(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let colors = colors(cx);
        let (maximize, label) = if window.is_maximized() {
            (IconName::WindowRestore, "还原窗口")
        } else {
            (IconName::WindowMaximize, "最大化")
        };
        div()
            .flex()
            .items_center()
            .gap(px(2.))
            .flex_shrink_0()
            .border_l_1()
            .border_color(rgb(colors.border))
            .pl(px(8.))
            .child(
                toolbar_icon("minimize-window", IconName::WindowMinimize, "最小化", cx)
                    .on_click(|_, window, _| window.minimize_window()),
            )
            .child(
                toolbar_icon("zoom-window", maximize, label, cx)
                    .on_click(|_, window, _| window.zoom_window()),
            )
            .child(
                toolbar_icon("close-window", IconName::WindowClose, "关闭窗口", cx)
                    .custom(variant(
                        cx,
                        colors.panel,
                        colors.ink,
                        colors.panel,
                        colors.error_fill,
                    ))
                    .on_click(|_, window, _| window.remove_window()),
            )
    }

    fn page_heading(&self, colors: Palette) -> Div {
        let (title, subtitle) = match self.page {
            Page::Task => ("密码恢复", "导入加密文件或已有哈希，选择候选密码的来源"),
            Page::Results => ("恢复结果", "查看本机保存的密码与对应文件"),
            Page::Tools => (
                "引擎工具",
                "查找当前 Hashcat 支持的类型和选项，运行自定义命令",
            ),
            Page::Forensics => ("磁盘取证", "读取证据镜像中的加密卷，用已知凭据导出解密副本"),
            Page::Settings => ("设置", "管理 Hashcat、下载资源、结果目录和外观"),
        };
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .flex_shrink_0()
            .px(px(24.))
            .pt(px(16.))
            .pb(px(16.))
            .child(
                div()
                    .text_size(px(26.))
                    .line_height(px(32.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(20.))
                    .text_color(rgb(colors.muted))
                    .child(subtitle),
            )
    }

    fn task_page(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = colors(cx);
        let stacked =
            window.viewport_size().width < px(940.) || window.viewport_size().height < px(620.);
        let active_log = self.cancel.is_some() || !self.logs.is_empty();
        div()
            .id("task-page")
            .flex()
            .flex_1()
            .min_h_0()
            .gap(px(16.))
            .px(px(24.))
            .pb(px(20.))
            .when(stacked, |this| this.flex_col().overflow_y_scroll())
            .when(!stacked && !active_log, |this| this.items_start())
            .child(self.task_form(stacked, cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .when(!stacked, |this| {
                        this.w(relative(0.355)).min_w(px(300.)).max_w(px(480.))
                    })
                    .when(stacked, |this| this.w_full().flex_shrink_0())
                    .when(stacked && active_log, |this| this.h(px(424.)))
                    .min_h_0()
                    .overflow_hidden()
                    .bg(rgb(colors.monitor))
                    .border_1()
                    .border_color(rgb(colors.border))
                    .rounded(px(12.))
                    .child(
                        self.run_status(!stacked && window.viewport_size().width < px(1080.), cx),
                    )
                    .when(!self.found_passwords.is_empty(), |this| {
                        this.child(self.inline_passwords(cx))
                    })
                    .child(self.log_panel(cx)),
            )
    }

    fn task_form(&self, stacked: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = colors(cx);
        let locked = self.form_locked();
        let action = if self.cancel.is_some() {
            button("stop", cx)
                .h(px(44.))
                .px(px(20.))
                .custom(variant(
                    cx,
                    colors.error_fill,
                    colors.error,
                    colors.border,
                    colors.error_fill,
                ))
                .child(div().size(px(10.)).rounded(px(2.)).bg(rgb(colors.error)))
                .child(div().text_color(rgb(colors.error)).child(
                    if self.phase == Phase::Stopping {
                        "正在停止"
                    } else {
                        "停止任务"
                    },
                ))
                .disabled(self.phase == Phase::Stopping)
                .on_click(cx.listener(|this, _, _, cx| this.stop_task(cx)))
        } else {
            let label = if self.installation.is_some() || self.probing {
                "开始恢复"
            } else {
                "连接 Hashcat"
            };
            primary("start", cx)
                .h(px(44.))
                .px(px(20.))
                .child(
                    div()
                        .text_color(rgb(colors.accent_ink))
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(label),
                )
                .child(icon(IconName::ArrowRight, colors.accent_ink, 16.))
                .disabled(self.probing || locked || self.picking)
                .on_click(cx.listener(|this, _, window, cx| this.start_task(window, cx)))
        };
        div()
            .flex()
            .flex_col()
            .when(!stacked, |this| this.flex_1().h_full())
            .when(stacked, |this| this.flex_shrink_0())
            .min_w_0()
            .min_h_0()
            .gap(px(16.))
            .child(
                div()
                    .id("form-body")
                    .flex()
                    .flex_col()
                    .min_h_0()
                    .when(!stacked, |this| this.overflow_y_scroll())
                    .gap(px(12.))
                    .child(
                        panel(colors)
                            .flex_shrink_0()
                            .p(px(20.))
                            .child(self.source_fields(cx)),
                    )
                    .child(
                        panel(colors)
                            .flex_shrink_0()
                            .p(px(20.))
                            .gap(px(12.))
                            .child(self.strategies())
                            .child(self.candidate_fields(cx))
                            .when(matches!(self.attack, 0 | 1 | 3 | 6 | 7 | 12), |this| {
                                this.child(self.quick_setup(cx))
                            }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .flex_shrink_0()
                    .when_some(self.form_error.as_ref(), |this, error| {
                        this.child(error_line(colors, error.clone()))
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(12.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .text_size(px(13.))
                                    .text_color(rgb(colors.muted))
                                    .child(icon(IconName::FolderClosed, colors.muted, 16.))
                                    .child("恢复结果保存在本机"),
                            )
                            .child(action),
                    ),
            )
    }

    fn source_fields(&self, cx: &mut Context<Self>) -> Div {
        let colors = colors(cx);
        let locked = self.form_locked();
        let choices = [
            ("source-protected", "加密文件", Source::Protected),
            ("source-hash", "哈希文件", Source::HashFile),
            ("source-paste", "粘贴哈希", Source::Paste),
        ];
        let mut group = div().flex().flex_col().gap(px(12.)).flex_shrink_0().child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .child(section_title("输入"))
                .child(div().flex().gap(px(4.)).children(choices.into_iter().map(
                    |(id, label, source)| {
                        choice(id, label, self.source == source, cx)
                            .h(px(32.))
                            .px(px(12.))
                            .disabled(locked || self.picking)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.source = source;
                                this.type_notice = None;
                                this.form_error = None;
                                cx.notify();
                            }))
                    },
                ))),
        );

        if self.source == Source::Protected {
            let message = match &self.extraction {
                Extraction::Idle => div()
                    .text_size(px(12.))
                    .text_color(rgb(colors.muted))
                    .child("ZIP、RAR、7z、PDF、DOCX、XLSX、PPTX"),
                Extraction::Pending => div()
                    .text_size(px(12.))
                    .text_color(rgb(colors.muted))
                    .child("正在读取加密信息…"),
                Extraction::Ready(data) => div()
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .text_size(px(12.))
                    .text_color(rgb(colors.success))
                    .child(icon(IconName::CircleCheck, colors.success, 14.))
                    .child(format!("{}  ·  Hashcat 模式 {}", data.format, data.mode)),
                Extraction::Failed(error) => error_line(colors, error.clone()),
            };
            return group
                .child(self.file_field(
                    "protected-file",
                    self.protected_file.as_deref(),
                    FileSlot::Protected,
                    "拖入加密文件",
                    cx,
                ))
                .child(message);
        }

        group = match self.source {
            Source::HashFile => group.child(self.file_field(
                "hash-file",
                self.hashes.as_deref(),
                FileSlot::Hashes,
                "选择包含哈希的文件",
                cx,
            )),
            Source::Paste => group.child(multiline(&self.hash_text, 80.).disabled(locked)),
            Source::Protected => unreachable!(),
        };
        let username_box = div()
            .flex()
            .items_center()
            .justify_center()
            .size(px(15.))
            .rounded(px(4.))
            .border_1()
            .border_color(rgb(if self.username {
                colors.selection_border
            } else {
                colors.border
            }))
            .bg(rgb(if self.username {
                colors.accent
            } else {
                colors.panel
            }))
            .when(self.username, |this| {
                this.child(icon(IconName::Check, colors.accent_ink, 12.))
            });
        group.child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .mt(px(2.))
                .child(field_label("哈希类型"))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div().min_w_0().flex_1().child(
                                Select::new(&self.modes)
                                    .w_full()
                                    .h(px(40.))
                                    .text_size(px(14.))
                                    .line_height(px(20.))
                                    .search_placeholder("搜索编号或算法名称")
                                    .disabled(locked || self.installation.is_none()),
                            ),
                        )
                        .child(
                            button("identify", cx)
                                .h(px(40.))
                                .px(px(10.))
                                .label(if self.identifying {
                                    "识别中"
                                } else {
                                    "识别类型"
                                })
                                .disabled(locked || self.picking || self.installation.is_none())
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.identify(window, cx)),
                                ),
                        ),
                )
                .when_some(self.type_notice.as_ref(), |this, note| {
                    this.child(notice(colors, note))
                })
                .child(
                    Button::new("has-username")
                        .text()
                        .justify_start()
                        .h(px(24.))
                        .gap(px(7.))
                        .disabled(locked)
                        .child(username_box)
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(rgb(colors.muted))
                                .child("输入含用户名"),
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.username = !this.username;
                            this.type_notice = None;
                            cx.notify();
                        })),
                ),
        )
    }

    fn strategies(&self) -> Div {
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .child(section_title("恢复方式"))
            .child(
                div().w(px(244.)).child(
                    Select::new(&self.attacks)
                        .w_full()
                        .h(px(40.))
                        .text_size(px(14.))
                        .line_height(px(20.))
                        .placeholder("选择恢复方式")
                        .search_placeholder("搜索名称或模式编号")
                        .disabled(self.form_locked() || self.installation.is_none()),
                ),
            )
    }

    fn candidate_fields(&self, cx: &mut Context<Self>) -> Div {
        let colors = colors(cx);
        let mut fields = div().flex().flex_col().gap(px(12.)).flex_shrink_0();
        if matches!(self.attack, 0 | 1 | 5 | 6 | 7 | 12) {
            let label = if self.attack == 1 {
                "第一份字典"
            } else {
                "字典文件"
            };
            fields = fields.child(field(
                label,
                self.file_field(
                    "wordlist",
                    self.words.as_deref(),
                    FileSlot::Words,
                    "选择候选密码字典",
                    cx,
                ),
            ));
        }
        if self.attack == 1 {
            fields = fields.child(field(
                "第二份字典",
                self.file_field(
                    "second-wordlist",
                    self.second_words.as_deref(),
                    FileSlot::SecondWords,
                    "选择第二份字典",
                    cx,
                ),
            ));
        }
        if matches!(self.attack, 3 | 6 | 7 | 12) {
            fields = fields.child(self.mask_field(cx));
        }
        let extra = match self.attack {
            0 | 1 | 3 | 6 | 7 => None,
            4 => Some((
                "PCFG 语法（可选）",
                "留空使用内置语法，也可指定一个语法目录或 .tar.xz 包",
            )),
            5 => Some(("变换表", "按使用顺序添加 .table 文件，每行一个")),
            8 => Some((
                "候选源模块与参数",
                "第一行填写本地 Feed 模块路径，其余参数每行一个",
            )),
            9 => Some((
                "关联字典（可选）",
                "每条哈希对应一行候选，也可由 Hashcat 从带标签的输入读取",
            )),
            12 => Some(("第二份字典（可选）", "掩码使用 ?q 时添加对应的字典")),
            _ => Some(("模式参数", "按此模式要求的顺序填写，每行一个参数")),
        };
        if let Some((label, hint)) = extra {
            fields = fields
                .child(field(
                    label,
                    multiline(&self.operands, 68.).disabled(self.form_locked()),
                ))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            button("operand-file", cx)
                                .h(px(32.))
                                .label("添加文件")
                                .disabled(self.form_locked() || self.picking)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.pick_file(FileSlot::OperandFile, window, cx)
                                })),
                        )
                        .when(matches!(self.attack, 4 | 8) || self.attack > 12, |row| {
                            row.child(
                                button("operand-dir", cx)
                                    .h(px(32.))
                                    .label("添加目录")
                                    .disabled(self.form_locked() || self.picking)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.pick_file(FileSlot::OperandDirectory, window, cx)
                                    })),
                            )
                        }),
                )
                .child(
                    div()
                        .text_size(px(13.))
                        .line_height(px(20.))
                        .text_color(rgb(colors.muted))
                        .child(hint),
                );
        }
        if matches!(self.attack, 0 | 9) {
            fields = fields.child(field(
                "规则文件（可选）",
                self.file_field(
                    "rule",
                    self.rule.as_deref(),
                    FileSlot::Rule,
                    "选择 .rule 文件",
                    cx,
                ),
            ));
        }
        fields
    }

    fn mask_field(&self, cx: &mut Context<Self>) -> Div {
        let colors = colors(cx);
        let row = if self.mask_file.is_some() {
            self.file_field(
                "mask-file",
                self.mask_file.as_deref(),
                FileSlot::Mask,
                "选择 .hcmask 文件",
                cx,
            )
        } else {
            div()
                .flex()
                .gap(px(8.))
                .items_center()
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .child(single_line(&self.mask_text).disabled(self.form_locked())),
                )
                .child(
                    button("mask-browse", cx)
                        .h(px(40.))
                        .label("掩码文件")
                        .disabled(self.form_locked() || self.picking)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.pick_file(FileSlot::Mask, window, cx)
                        })),
                )
        };
        field("掩码", row).child(
            div()
                .text_size(px(13.))
                .line_height(px(20.))
                .text_color(rgb(colors.muted))
                .child(if self.attack == 12 {
                    "?w 第一份字典　?q 第二份字典　?d 数字"
                } else {
                    "?d 数字　?l 小写　?u 大写　?s 符号"
                }),
        )
    }

    fn file_field(
        &self,
        id: &'static str,
        path: Option<&std::path::Path>,
        slot: FileSlot,
        hint: &'static str,
        cx: &mut Context<Self>,
    ) -> Div {
        let colors = colors(cx);
        let locked = self.form_locked()
            || self.picking
            || (matches!(slot, FileSlot::Executable)
                && (self.probing || matches!(self.devices, DeviceQuery::Pending)));
        let featured = matches!(slot, FileSlot::Protected);
        let leading = icon(
            IconName::File,
            if featured { colors.ink } else { colors.muted },
            16.,
        );
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .id((id, 2_u32))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(if featured { 56. } else { 40. }))
                    .flex_1()
                    .min_w_0()
                    .px(px(if featured { 16. } else { 12. }))
                    .rounded(px(8.))
                    .bg(rgb(colors.raised))
                    .border_1()
                    .border_color(rgb(colors.border))
                    .when(!locked && !matches!(slot, FileSlot::Executable), |this| {
                        this.drag_over::<ExternalPaths>(move |style, _, _, _| {
                            style
                                .bg(rgb(colors.selection))
                                .border_color(rgb(colors.selection_border))
                        })
                        .on_drop(cx.listener(
                            move |this, paths: &ExternalPaths, window, cx| {
                                if let [path] = paths.paths() {
                                    this.set_file(slot, path.clone(), window, cx);
                                } else {
                                    this.form_error = Some("请一次拖入一个文件。".into());
                                    cx.notify();
                                }
                            },
                        ))
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .flex_shrink_0()
                            .when(featured, |this| {
                                this.size(px(32.))
                                    .rounded(px(8.))
                                    .bg(rgb(colors.panel))
                                    .border_1()
                                    .border_color(rgb(colors.border))
                            })
                            .child(leading),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .text_ellipsis()
                            .text_size(px(if featured { 15. } else { 13. }))
                            .when(featured, |this| this.font_weight(FontWeight::MEDIUM))
                            .text_color(rgb(if path.is_some() {
                                colors.ink
                            } else {
                                colors.muted
                            }))
                            .child(path.map_or_else(|| hint.into(), hashcat::file_name)),
                    )
                    .when(
                        path.is_some() && matches!(slot, FileSlot::Rule | FileSlot::Mask),
                        |this| {
                            this.child(
                                Button::new((id, 1_u32))
                                    .text()
                                    .small()
                                    .icon(IconName::Close)
                                    .tooltip("清除文件")
                                    .ml_auto()
                                    .disabled(locked)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        match slot {
                                            FileSlot::Rule => this.rule = None,
                                            FileSlot::Mask => this.mask_file = None,
                                            _ => {}
                                        }
                                        this.form_error = None;
                                        cx.notify();
                                    })),
                            )
                        },
                    ),
            )
            .child(
                button((id, 0_u32), cx)
                    .h(px(40.))
                    .flex_shrink_0()
                    .px(px(12.))
                    .child(icon(IconName::Folder, colors.muted, 16.))
                    .child(if path.is_some() {
                        "更换"
                    } else if featured {
                        "选择文件"
                    } else {
                        "浏览"
                    })
                    .disabled(locked)
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.pick_file(slot, window, cx)),
                    ),
            )
    }

    fn run_status(&self, narrow: bool, cx: &mut Context<Self>) -> Div {
        let colors = colors(cx);
        let status_color = match self.phase {
            Phase::Starting | Phase::Running | Phase::Stopping => colors.ink,
            Phase::Recovered => colors.success,
            Phase::Failed => colors.error,
            _ => colors.muted,
        };
        let share = self.status.progress.and_then(|[done, total]| {
            (total > 0).then(|| (done as f64 / total as f64).clamp(0., 1.))
        });
        let percent = share.map_or_else(|| "—".to_owned(), |share| format!("{:.1}%", share * 100.));
        let recovered = self.status.recovered_hashes.map_or_else(
            || "—".to_owned(),
            |[count, total]| format!("{count} / {total}"),
        );
        let speed = if self.cancel.is_some() && !self.status.devices.is_empty() {
            let sum = self
                .status
                .devices
                .iter()
                .fold(0_u64, |total, device| total.saturating_add(device.speed));
            display_speed(sum)
        } else {
            "—".to_owned()
        };
        let eta = if self.phase == Phase::Running {
            self.status
                .estimated_stop
                .filter(|time| *time > 0)
                .and_then(|time| DateTime::from_timestamp(time, 0))
                .map(|time| time.with_timezone(&Local).format("%H:%M:%S").to_string())
        } else {
            None
        };
        div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .p(px(20.))
            .gap(px(12.))
            .text_color(rgb(colors.ink))
            .border_b_1()
            .border_color(rgb(colors.border))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(section_title("当前任务"))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(px(13.))
                            .text_color(rgb(status_color))
                            .child(dot(status_color))
                            .child(self.phase.label()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .items_center()
                            .text_size(px(13.))
                            .text_color(rgb(colors.muted))
                            .child("进度")
                            .child(
                                div()
                                    .text_color(rgb(colors.ink))
                                    .font_family("Cascadia Mono")
                                    .child(percent),
                            ),
                    )
                    .child(
                        div()
                            .h(px(8.))
                            .w_full()
                            .rounded_full()
                            .bg(rgb(colors.raised))
                            .child(
                                div()
                                    .h_full()
                                    .w(relative(share.unwrap_or(0.) as f32))
                                    .rounded_full()
                                    .bg(rgb(colors.accent)),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .min_w_0()
                    .when(narrow, |this| this.flex_col().gap(px(4.)))
                    .child(metric(colors, "已恢复", recovered, false, narrow))
                    .child(metric(colors, "速度", speed, true, narrow))
                    .child(metric(
                        colors,
                        "预计完成",
                        eta.unwrap_or_else(|| "—".to_owned()),
                        true,
                        narrow,
                    )),
            )
            .when_some(self.run_error.as_ref(), |this, error| {
                this.child(
                    div()
                        .text_size(px(12.))
                        .text_color(rgb(colors.error))
                        .child(error.clone()),
                )
            })
            .when_some(
                self.current_result.as_ref().filter(|_| {
                    self.cancel.is_none()
                        && self
                            .status
                            .recovered_hashes
                            .is_some_and(|[count, _]| count > 6)
                }),
                |this, path| {
                    let path = path.clone();
                    this.child(
                        button("show-current-result", cx)
                            .h(px(36.))
                            .w_full()
                            .custom(variant(
                                cx,
                                colors.raised,
                                colors.ink,
                                colors.border,
                                colors.selection,
                            ))
                            .child(icon(IconName::FolderOpen, colors.ink, 16.))
                            .child(div().text_color(rgb(colors.ink)).child("查看全部结果"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.selected_result = Some(path.clone());
                                this.change_page(Page::Results, cx);
                            })),
                    )
                },
            )
    }

    fn inline_passwords(&self, cx: &mut Context<Self>) -> Div {
        let c = colors(cx);
        div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .gap(px(6.))
            .px(px(20.))
            .py(px(12.))
            .bg(rgb(c.selection))
            .border_b_1()
            .border_color(rgb(c.border))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(section_title("已恢复密码"))
                    .child(
                        button("copy-passwords", cx)
                            .h(px(28.))
                            .label("复制")
                            .on_click(cx.listener(|this, _, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(
                                    this.found_passwords
                                        .iter()
                                        .rev()
                                        .map(String::as_str)
                                        .collect::<Vec<_>>()
                                        .join("\n"),
                                ));
                            })),
                    ),
            )
            .child(
                gpui::uniform_list(
                    "inline-passwords",
                    self.found_passwords.len(),
                    cx.processor(|this, range: std::ops::Range<usize>, _, _| {
                        range
                            .filter_map(|i| this.found_passwords.iter().rev().nth(i))
                            .map(|v| {
                                div()
                                    .h(px(24.))
                                    .font_family("Cascadia Mono")
                                    .text_size(px(15.))
                                    .line_height(px(22.))
                                    .whitespace_nowrap()
                                    .child(if v.is_empty() {
                                        "（空密码）".to_owned()
                                    } else {
                                        v.clone()
                                    })
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .with_horizontal_sizing_behavior(gpui::ListHorizontalSizingBehavior::Unconstrained)
                .h(px(24. * self.found_passwords.len().min(5) as f32)),
            )
    }

    fn log_panel(&self, cx: &mut Context<Self>) -> Div {
        let colors = colors(cx);
        let copy_color = colors.muted;
        div()
            .flex()
            .flex_col()
            .when(self.logs.is_empty() && self.cancel.is_none(), |this| {
                this.flex_none()
            })
            .when(!self.logs.is_empty() || self.cancel.is_some(), |this| {
                this.flex_1()
            })
            .min_h_0()
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .h(px(44.))
                    .px(px(20.))
                    .flex_shrink_0()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_color(rgb(colors.ink))
                            .text_size(px(14.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(icon(IconName::SquareTerminal, colors.muted, 16.))
                            .child("运行日志"),
                    )
                    .child(
                        button("copy-log", cx)
                            .h(px(28.))
                            .px(px(8.))
                            .custom(variant(
                                cx,
                                colors.monitor,
                                copy_color,
                                colors.monitor,
                                colors.raised,
                            ))
                            .child(icon(
                                if self.copied_log {
                                    IconName::Check
                                } else {
                                    IconName::Copy
                                },
                                copy_color,
                                14.,
                            ))
                            .child(div().text_size(px(12.)).text_color(rgb(copy_color)).child(
                                if self.copied_log {
                                    "已复制"
                                } else {
                                    "复制"
                                },
                            ))
                            .disabled(self.logs.is_empty())
                            .on_click(cx.listener(|this, _, _, cx| {
                                let text = this
                                    .logs
                                    .iter()
                                    .map(|line| line.as_ref())
                                    .collect::<Vec<_>>()
                                    .join("\n");
                                cx.write_to_clipboard(ClipboardItem::new_string(text));
                                this.copied_log = true;
                                cx.notify();
                            })),
                    ),
            )
            .child(if self.logs.is_empty() {
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(20.))
                    .pb(px(20.))
                    .text_size(px(13.))
                    .text_color(rgb(colors.muted))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(px(20.))
                            .child(icon(IconName::SquareTerminal, colors.muted, 16.)),
                    )
                    .child(if self.cancel.is_some() {
                        "正在等待 Hashcat 输出"
                    } else {
                        "启动任务后显示日志"
                    })
                    .into_any_element()
            } else {
                div()
                    .id("log-scroller")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.log_scroll)
                    .px(px(20.))
                    .pb(px(16.))
                    .gap(px(4.))
                    .font_family("Cascadia Mono")
                    .text_size(px(13.))
                    .line_height(px(20.))
                    .text_color(rgb(colors.muted))
                    .children(
                        self.logs
                            .iter()
                            .cloned()
                            .map(|line| div().min_w_0().flex_shrink_0().child(line)),
                    )
                    .into_any_element()
            })
    }

    fn results_page(&self, cx: &mut Context<Self>) -> Div {
        let colors = colors(cx);
        let files = panel(colors)
            .w(px(244.))
            .min_h_0()
            .flex_shrink_0()
            .child(panel_header(colors, "本地记录", IconName::FolderOpen))
            .child(
                div()
                    .id("results-list")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(px(12.))
                    .gap(px(5.))
                    .when(self.results.is_empty(), |this| {
                        this.child(
                            div()
                                .text_size(px(13.))
                                .text_color(rgb(colors.muted))
                                .px(px(8.))
                                .py(px(7.))
                                .child("暂无已恢复的结果"),
                        )
                    })
                    .children(self.results.iter().enumerate().map(|(index, file)| {
                        let path = file.path.clone();
                        let time: DateTime<Local> = file.modified.into();
                        let selected = self.selected_result.as_ref() == Some(&path);
                        let fill = if selected {
                            colors.selection
                        } else {
                            colors.panel
                        };
                        button(("result-row", index), cx)
                            .custom(variant(cx, fill, colors.ink, colors.border, colors.raised))
                            .h(px(58.))
                            .w_full()
                            .justify_start()
                            .px(px(10.))
                            .gap(px(8.))
                            .child(icon(IconName::File, colors.success, 14.))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_w_0()
                                    .gap(px(3.))
                                    .child(div().text_ellipsis().child(file.source.clone()))
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(rgb(colors.muted))
                                            .child(time.format("%Y-%m-%d  %H:%M:%S").to_string()),
                                    ),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.select_result(Some(path.clone()));
                                cx.notify();
                            }))
                    })),
            );
        let display = panel(colors)
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(10.))
                    .px(px(17.))
                    .h(px(52.))
                    .flex_shrink_0()
                    .border_b_1()
                    .border_color(rgb(colors.border))
                    .child(section_title("结果预览"))
                    .when_some(self.selected_result.as_ref(), |this, path| {
                        let path = path.clone();
                        this.child(
                            button("reveal-result", cx)
                                .h(px(31.))
                                .px(px(9.))
                                .gap(px(6.))
                                .child(icon(IconName::ExternalLink, colors.muted, 14.))
                                .child("在文件夹中查看")
                                .on_click(move |_, _, cx| cx.reveal_path(&path)),
                        )
                    }),
            )
            .child(match &self.result_preview {
                Some(Ok((content, truncated))) => div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .id("result-preview")
                            .flex_1()
                            .min_h_0()
                            .overflow_scroll()
                            .track_scroll(&self.results_scroll)
                            .p(px(18.))
                            .font_family("Cascadia Mono")
                            .text_size(px(12.))
                            .line_height(px(20.))
                            .child(div().whitespace_nowrap().child(content.clone())),
                    )
                    .child(
                        div()
                            .px(px(18.))
                            .py(px(10.))
                            .border_t_1()
                            .border_color(rgb(colors.border))
                            .text_size(px(12.))
                            .text_color(rgb(colors.muted))
                            .child(if *truncated {
                                "预览已截断。请打开文件查看完整结果。"
                            } else {
                                "结果文件包含恢复出的密码。"
                            }),
                    )
                    .into_any_element(),
                Some(Err(error)) => div()
                    .p(px(18.))
                    .child(error_line(colors, error.clone()))
                    .into_any_element(),
                None => div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap(px(10.))
                    .text_color(rgb(colors.muted))
                    .child(icon(IconName::File, colors.muted, 29.))
                    .child("找到密码后，结果会出现在这里。")
                    .into_any_element(),
            });
        div()
            .flex()
            .flex_1()
            .min_h_0()
            .gap(px(16.))
            .px(px(24.))
            .pb(px(24.))
            .child(files)
            .child(display)
    }

    fn environment_page(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = colors(cx);
        let details = match &self.devices {
            DeviceQuery::Idle => div()
                .text_color(rgb(colors.muted))
                .child("点击检测设备，查看可用的后端、显卡与驱动。"),
            DeviceQuery::Pending => div()
                .text_color(rgb(colors.muted))
                .child("正在等待 Hashcat 返回设备信息…"),
            DeviceQuery::Ready(text) => div()
                .font_family("Cascadia Mono")
                .text_size(px(12.))
                .line_height(px(18.))
                .child(text.clone()),
            DeviceQuery::Failed(text) => error_line(colors, text.clone()),
        };
        let version = self.installation.as_ref().map(|installation| {
            format!(
                "{} · {} 种哈希模式",
                installation.version,
                installation.modes.len()
            )
        });
        div()
            .id("settings-scroller")
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .gap(px(16.))
            .px(px(24.))
            .pb(px(24.))
            .child(
                panel(colors)
                    .flex_shrink_0()
                    .child(panel_header(colors, "Hashcat 程序", IconName::Settings))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .p(px(19.))
                            .gap(px(9.))
                            .child(self.file_field(
                                "engine-location",
                                self.executable.as_deref(),
                                FileSlot::Executable,
                                "选择 Hashcat 8 的 hashcat.exe",
                                cx,
                            ))
                            .when_some(self.executable.as_ref(), |this, path| {
                                this.child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(rgb(colors.muted))
                                        .child(path.display().to_string()),
                                )
                            })
                            .when_some(version, |this, version| {
                                this.child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(rgb(colors.success))
                                        .child(version),
                                )
                            })
                            .when_some(self.engine_notice.as_ref(), |this, note| {
                                this.child(notice(colors, note))
                            }),
                    ),
            )
            .child(self.storage_panel(cx))
            .child(self.download_panel(cx))
            .child(self.appearance_panel(cx))
            .child(
                panel(colors)
                    .flex_shrink_0()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .h(px(52.))
                            .flex_shrink_0()
                            .px(px(18.))
                            .border_b_1()
                            .border_color(rgb(colors.border))
                            .child(section_title("计算设备"))
                            .child(
                                button("refresh-devices", cx)
                                    .h(px(31.))
                                    .px(px(10.))
                                    .gap(px(6.))
                                    .child(icon(IconName::Search, colors.success, 14.))
                                    .child("检测设备")
                                    .disabled(
                                        self.installation.is_none()
                                            || self.form_locked()
                                            || matches!(self.devices, DeviceQuery::Pending),
                                    )
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.refresh_devices(window, cx)
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .id("devices-details")
                            .max_h(px(320.))
                            .overflow_y_scroll()
                            .p(px(19.))
                            .child(details),
                    ),
            )
    }

    fn appearance_panel(&self, cx: &mut Context<Self>) -> Div {
        let colors = colors(cx);
        let appearances = [
            ("theme-light", "暖白", Appearance::Light),
            ("theme-mint", "薄荷", Appearance::Mint),
            ("theme-dark", "炭黑", Appearance::Dark),
            ("theme-night", "午夜蓝", Appearance::Midnight),
        ];
        let outlines = [
            ("outline-cat", "猫咪头", Outline::Cat),
            ("outline-cut", "斜切", Outline::Cut),
            ("outline-rounded", "圆角", Outline::Rounded),
            ("outline-standard", "标准", Outline::Standard),
        ];
        panel(colors)
            .p(px(20.))
            .gap(px(12.))
            .flex_shrink_0()
            .child(section_title("外观"))
            .child(
                div()
                    .flex()
                    .items_center()
                    .flex_wrap()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(field_label("主题"))
                            .children(appearances.into_iter().map(|(id, label, appearance)| {
                                choice(id, label, self.preferences.appearance == appearance, cx)
                                    .h(px(32.))
                                    .px(px(12.))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.set_appearance(appearance, window, cx)
                                    }))
                            })),
                    )
                    .when(cfg!(windows), |this| {
                        this.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(12.))
                                .child(field_label("窗口轮廓"))
                                .children(outlines.into_iter().map(|(id, label, outline)| {
                                    choice(id, label, self.preferences.outline == outline, cx)
                                        .h(px(32.))
                                        .px(px(12.))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.set_outline(outline, window, cx)
                                        }))
                                })),
                        )
                    }),
            )
            .when_some(self.appearance_error.as_ref(), |this, error| {
                this.child(error_line(colors, error.clone()))
            })
    }
}

pub(super) fn single_line(state: &Entity<InputState>) -> Input {
    // Input::h is a multiline option. Set the actual single-line frame through Styled.
    Styled::h(Input::new(state), px(40.))
        .w_full()
        .px(px(12.))
        .py_0()
        .text_size(px(14.))
        .line_height(px(20.))
}

pub(super) fn multiline(state: &Entity<InputState>, height: f32) -> Input {
    Input::new(state)
        .w_full()
        .h(px(height))
        .px(px(12.))
        .py(px(8.))
        .text_size(px(14.))
        .line_height(px(20.))
}

pub(super) fn panel(colors: Palette) -> Div {
    div()
        .flex()
        .flex_col()
        .bg(rgb(colors.panel))
        .border_1()
        .border_color(rgb(colors.border))
        .rounded(px(12.))
}

fn panel_header(colors: Palette, title: &'static str, glyph: IconName) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(9.))
        .h(px(51.))
        .px(px(19.))
        .flex_shrink_0()
        .border_b_1()
        .border_color(rgb(colors.border))
        .child(icon(glyph, colors.success, 16.))
        .child(section_title(title))
}

pub(super) fn section_title(title: &'static str) -> Div {
    div()
        .text_size(px(16.))
        .line_height(px(24.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(title)
}

fn field_label(text: &'static str) -> Div {
    div()
        .text_size(px(14.))
        .line_height(px(20.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(text)
}

fn field(label: &'static str, item: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(field_label(label))
        .child(item)
}

fn icon(name: IconName, color: u32, size: f32) -> Icon {
    Icon::new(name).size(px(size)).text_color(rgb(color))
}

fn dot(color: u32) -> Div {
    div()
        .size(px(6.))
        .flex_shrink_0()
        .rounded_full()
        .bg(rgb(color))
}

fn variant(cx: &App, fill: u32, ink: u32, border: u32, hover: u32) -> ButtonCustomVariant {
    ButtonCustomVariant::new(cx)
        .color(rgb(fill).into())
        .foreground(rgb(ink).into())
        .border(rgb(border).into())
        .hover(rgb(hover).into())
        .active(rgb(hover).into())
}

pub(super) fn button(id: impl Into<ElementId>, cx: &App) -> Button {
    let colors = colors(cx);
    Button::new(id)
        .small()
        .custom(variant(
            cx,
            colors.panel,
            colors.ink,
            colors.border,
            colors.raised,
        ))
        .text_color(rgb(colors.ink))
        .text_size(px(13.))
        .font_weight(FontWeight::MEDIUM)
}

fn toolbar_icon(id: &'static str, glyph: IconName, label: &'static str, cx: &App) -> Button {
    let colors = colors(cx);
    button(id, cx)
        .custom(variant(
            cx,
            colors.panel,
            colors.muted,
            colors.panel,
            colors.raised,
        ))
        .w(px(32.))
        .h(px(36.))
        .p_0()
        .child(icon(glyph, colors.muted, 16.))
        .tooltip(label)
}

pub(super) fn primary(id: &'static str, cx: &App) -> Button {
    let colors = colors(cx);
    button(id, cx).custom(variant(
        cx,
        colors.accent,
        colors.accent_ink,
        colors.selection_border,
        colors.accent_hover,
    ))
}

pub(super) fn choice(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
    cx: &App,
) -> Button {
    let colors = colors(cx);
    let (fill, ink, border) = if selected {
        (colors.selection, colors.ink, colors.selection_border)
    } else {
        (colors.panel, colors.muted, colors.border)
    };
    button(id, cx)
        .custom(variant(cx, fill, ink, border, colors.raised))
        .child(
            div()
                .text_size(px(13.))
                .text_color(rgb(ink))
                .child(label.into()),
        )
}

fn error_line(colors: Palette, text: String) -> Div {
    div()
        .text_size(px(12.))
        .line_height(px(17.))
        .text_color(rgb(colors.error))
        .bg(rgb(colors.error_fill))
        .px(px(8.))
        .py(px(5.))
        .rounded(px(6.))
        .child(text)
}

fn notice(colors: Palette, note: &Notice) -> AnyElement {
    if note.error {
        error_line(colors, note.text.clone()).into_any_element()
    } else {
        div()
            .text_size(px(12.))
            .text_color(rgb(colors.success))
            .child(note.text.clone())
            .into_any_element()
    }
}

fn metric(colors: Palette, label: &'static str, value: String, divided: bool, narrow: bool) -> Div {
    div()
        .flex()
        .when(!narrow, |this| this.flex_col().flex_1())
        .when(narrow, |this| this.items_center().justify_between())
        .min_w_0()
        .gap(px(4.))
        .when(divided && !narrow, |this| {
            this.pl(px(12.))
                .border_l_1()
                .border_color(rgb(colors.border))
        })
        .child(
            div()
                .text_size(px(12.))
                .text_color(rgb(colors.muted))
                .child(label),
        )
        .child(
            div()
                .text_size(px(16.))
                .line_height(px(22.))
                .font_weight(FontWeight::MEDIUM)
                .font_family("Cascadia Mono")
                .text_color(rgb(colors.ink))
                .text_ellipsis()
                .child(value),
        )
}

fn display_speed(speed: u64) -> String {
    if speed < 1_000 {
        format!("{speed} H/s")
    } else if speed < 1_000_000 {
        format!("{:.1} kH/s", speed as f64 / 1_000.)
    } else if speed < 1_000_000_000 {
        format!("{:.1} MH/s", speed as f64 / 1_000_000.)
    } else {
        format!("{:.1} GH/s", speed as f64 / 1_000_000_000.)
    }
}

fn logo() -> impl IntoElement {
    static IMAGE: LazyLock<Arc<gpui::Image>> = LazyLock::new(|| {
        Arc::new(gpui::Image::from_bytes(
            gpui::ImageFormat::Png,
            include_bytes!("../../assets/lovelycat-toolbar.png").to_vec(),
        ))
    });
    gpui::img(Arc::clone(&IMAGE)).size(px(40.)).flex_shrink_0()
}

fn cat_surface(colors: Palette) -> impl IntoElement {
    canvas(
        |bounds, _, _| bounds,
        move |_, bounds, window, _| {
            let width = f32::from(bounds.size.width);
            let height = f32::from(bounds.size.height);
            let mut builder = gpui::PathBuilder::fill();
            for (i, (x, y)) in crate::desktop::cat_contour(width, height, CAT_EAR_HEIGHT / 2.)
                .into_iter()
                .enumerate()
            {
                let pt = bounds.origin + point(px(x), px(y));
                if i == 0 {
                    builder.move_to(pt);
                } else {
                    builder.line_to(pt);
                }
            }
            builder.close();
            if let Ok(shape) = builder.build() {
                let split = px(CAT_EAR_HEIGHT + 56.);
                let head = gpui::Bounds::new(bounds.origin, gpui::size(bounds.size.width, split));
                let body = gpui::Bounds::new(
                    bounds.origin + point(px(0.), split),
                    gpui::size(bounds.size.width, bounds.size.height - split),
                );
                window.with_content_mask(Some(gpui::ContentMask { bounds: head }), |w| {
                    w.paint_path(shape.clone(), rgb(colors.panel))
                });
                window.with_content_mask(Some(gpui::ContentMask { bounds: body }), |w| {
                    w.paint_path(shape, rgb(colors.background))
                });
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

fn cat_ears(colors: Palette) -> impl IntoElement {
    div()
        .id("cat-ears")
        .w_full()
        .h(px(CAT_EAR_HEIGHT))
        .flex_shrink_0()
        .window_control_area(WindowControlArea::Drag)
        .child(
            canvas(
                |bounds, _, _| bounds,
                move |_, bounds, window, _| {
                    for right in [false, true] {
                        let point_at = |x: f32, y: f32| {
                            let x = if right {
                                bounds.size.width - px(x)
                            } else {
                                px(x)
                            };
                            bounds.origin + point(x, px(y))
                        };
                        let mut ear = gpui::PathBuilder::fill();
                        ear.move_to(point_at(31., 15.));
                        ear.line_to(point_at(67., 33.));
                        ear.line_to(point_at(23., 33.));
                        ear.close();
                        if let Ok(path) = ear.build() {
                            window.paint_path(path, rgb(colors.selection));
                        }
                    }
                },
            )
            .size_full(),
        )
}

#[cfg(all(test, windows))]
mod layout_tests {
    use super::*;
    use gpui::{
        AnyWindowHandle, AppContext, Application, Bounds, EntityInputHandler, WindowBounds,
        WindowOptions, size,
    };
    use gpui_component::Root;
    use std::{cell::Cell, rc::Rc};

    struct Sample {
        input: Entity<InputState>,
        frame: Rc<Cell<Option<Bounds<gpui::Pixels>>>>,
    }

    impl Render for Sample {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let frame = self.frame.clone();
            div()
                .flex()
                .w_full()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .relative()
                        .flex_1()
                        .min_w_0()
                        .child(single_line(&self.input))
                        .child(
                            canvas(
                                |bounds, _, _| bounds,
                                move |_, bounds, _, _| frame.set(Some(bounds)),
                            )
                            .absolute()
                            .inset_0(),
                        ),
                )
                .child(button("browse", cx).label("掩码文件").h(px(40.)))
        }
    }

    #[test]
    #[ignore = "measures actual GPUI text layout with the Windows renderer, in hidden windows"]
    fn mask_text_fits_its_40_pixel_field() {
        Application::headless()
            .with_assets(gpui_component_assets::Assets)
            .run(|cx| {
                gpui_component::init(cx);
                for (width, appearance) in [
                    (1200., Appearance::Light),
                    (960., Appearance::Dark),
                    (760., Appearance::Mint),
                    (720., Appearance::Midnight),
                ] {
                    super::super::configure_theme(appearance, cx);
                    let frame = Rc::new(Cell::new(None));
                    let mut field = None;
                    let handle = cx
                        .open_window(
                            WindowOptions {
                                show: false,
                                focus: false,
                                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                                    None,
                                    size(px(width), px(400.)),
                                    cx,
                                ))),
                                ..Default::default()
                            },
                            |window, cx| {
                                let input = cx.new(|cx| InputState::new(window, cx));
                                input.update(cx, |s, cx| s.set_value("例如 ?d?d?d?d", window, cx));
                                field = Some(input.clone());
                                let sample = cx.new(|_| Sample {
                                    input,
                                    frame: frame.clone(),
                                });
                                cx.new(|cx| Root::new(sample, window, cx))
                            },
                        )
                        .unwrap();
                    AnyWindowHandle::from(handle)
                        .update(cx, |_, window, cx| {
                            window.draw(cx).clear();
                            let field_bounds = frame.get().expect("field was laid out");
                            let glyphs = field
                                .as_ref()
                                .unwrap()
                                .update(cx, |s, cx| {
                                    s.bounds_for_range(
                                        0.."例如 ?d?d?d?d".encode_utf16().count(),
                                        Bounds::default(),
                                        window,
                                        cx,
                                    )
                                })
                                .unwrap();
                            assert_eq!(field_bounds.size.height, px(40.));
                            assert!(
                                glyphs.size.height >= px(18.) && glyphs.size.height <= px(22.),
                                "{glyphs:?}"
                            );
                            assert!(
                                glyphs.size.width >= px(50.) && glyphs.size.width < px(120.),
                                "{glyphs:?}"
                            );
                            window.remove_window();
                        })
                        .unwrap();
                    let local = tempfile::tempdir().unwrap();
                    let mut view = None;
                    let workbench = cx
                        .open_window(
                            WindowOptions {
                                show: false,
                                focus: false,
                                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                                    None,
                                    size(px(width), px(740.)),
                                    cx,
                                ))),
                                ..Default::default()
                            },
                            |window, cx| {
                                let preferences = crate::desktop::Preferences {
                                    appearance,
                                    outline: Outline::Cat,
                                };
                                let bench = cx.new(|cx| {
                                    Workbench::new(local.path().into(), preferences, window, cx)
                                });
                                view = Some(bench.clone());
                                cx.new(|cx| Root::new(bench, window, cx))
                            },
                        )
                        .unwrap();
                    AnyWindowHandle::from(workbench)
                        .update(cx, |_, window, cx| {
                            let bench = view.as_ref().unwrap();
                            for page in [Page::Task, Page::Tools, Page::Results, Page::Settings] {
                                bench.update(cx, |b, cx| {
                                    b.attack = 3;
                                    b.found_passwords = vec!["layout sample".into()];
                                    b.page = page;
                                    cx.notify();
                                });
                                window.refresh();
                                window.draw(cx).clear();
                            }
                            window.remove_window();
                        })
                        .unwrap();
                }
                cx.quit();
            });
    }
}
