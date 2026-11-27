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
    scroll::ScrollableElement,
    select::Select,
};

use super::{
    DeviceQuery, Extraction, FileSlot, Notice, Page, Phase, Source, UpdateState, Workbench, hashcat,
};

#[cfg(windows)]
use super::palette::reset_root_background;
use super::palette::{Palette, colors, set_widget_background, theme_name, theme_swatch};
use crate::desktop::{Appearance, CAT_EAR_HEIGHT, Outline};

impl Render for Workbench {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = colors(cx);
        // Root::render already painted transparent beneath Windows cat ears.
        // Switch Theme.background to an opaque panel surface so child widgets
        // (Select popovers, search inputs, text fields, Tooltips) never turn transparent.
        set_widget_background(colors, cx);
        let cat = cfg!(windows)
            && self.preferences.outline == Outline::Cat
            && !window.is_maximized()
            && !window.is_fullscreen();
        div()
            .id("workbench-root")
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .on_key_down(cx.listener(Self::handle_key_down))
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
                    .child(self.page_heading(cx))
                    .child(match self.page {
                        Page::Task => self.task_page(window, cx).into_any_element(),
                        Page::Tools => self.tools_page(window, cx).into_any_element(),
                        Page::Results => self.results_page(window, cx).into_any_element(),
                        Page::Forensics => self.forensics_page(window, cx).into_any_element(),
                        Page::Settings => self.environment_page(window, cx).into_any_element(),
                    }),
            )
            .when(cfg!(windows), |this| {
                #[cfg(windows)]
                {
                    this.child(restore_root_background())
                }
                #[cfg(not(windows))]
                {
                    this
                }
            })
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
                "恢复任务 (Ctrl+1)",
                IconName::LayoutDashboard,
            ),
            (
                Page::Results,
                "nav-results",
                "恢复结果",
                "恢复结果 (Ctrl+2)",
                IconName::FolderOpen,
            ),
            (
                Page::Tools,
                "nav-tools",
                "引擎工具",
                "引擎工具 (Ctrl+3)",
                IconName::SquareTerminal,
            ),
            (
                Page::Forensics,
                "nav-forensics",
                "磁盘取证",
                "磁盘取证 (Ctrl+4)",
                IconName::Inspector,
            ),
            (
                Page::Settings,
                "nav-engine",
                "设置",
                "设置 (Ctrl+5)",
                IconName::Settings2,
            ),
        ];
        let next_mode = self.preferences.appearance.toggle();
        let theme_icon = match next_mode {
            Appearance::Dark | Appearance::Midnight => IconName::Moon,
            _ => IconName::Sun,
        };
        let theme_label = format!("切换到{}", theme_name(next_mode));
        let task_running = self.cancel.is_some();
        div()
            .flex()
            .items_center()
            .h(px(56.))
            .flex_shrink_0()
            .px(px(if compact { 12. } else { 24. }))
            .gap(px(if narrow {
                6.
            } else if compact {
                10.
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
                                .text_size(px(19.))
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
                    .gap(px(2.))
                    .p(px(3.))
                    .rounded(px(10.))
                    .bg(rgb(colors.background))
                    .border_1()
                    .border_color(rgb(colors.subtle_border))
                    .children(nav.into_iter().map(|(page, id, title, tip, glyph)| {
                        let selected = self.page == page;
                        let fill = if selected {
                            colors.panel
                        } else {
                            colors.background
                        };
                        let border = if selected {
                            colors.border
                        } else {
                            colors.background
                        };
                        let hover = if selected {
                            colors.panel
                        } else {
                            colors.raised
                        };
                        let foreground = if selected { colors.ink } else { colors.muted };
                        let show_running_dot = task_running && page == Page::Task;
                        button(id, cx)
                            .custom(variant(cx, fill, foreground, border, hover))
                            .h(px(32.))
                            .px(px(if narrow {
                                6.
                            } else if compact {
                                8.
                            } else {
                                11.
                            }))
                            .gap(px(6.))
                            .tooltip(tip)
                            .child(icon(glyph, foreground, 15.))
                            .child(
                                div()
                                    .text_size(px(if narrow { 13. } else { 13.5 }))
                                    .font_weight(if selected {
                                        FontWeight::SEMIBOLD
                                    } else {
                                        FontWeight::MEDIUM
                                    })
                                    .text_color(rgb(foreground))
                                    .child(title),
                            )
                            .when(show_running_dot, |btn| btn.child(dot(colors.success)))
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
                    .when_some(
                        match &self.update {
                            UpdateState::Available(r) => Some(format!("v{}", r.version)),
                            UpdateState::Ready(p) => Some(format!("重启至 v{}", p.version)),
                            _ => None,
                        },
                        |row, label| {
                            row.child(
                                button("toolbar-update-badge", cx)
                                    .custom(variant(
                                        cx,
                                        colors.success_fill,
                                        colors.success,
                                        colors.border,
                                        colors.selection,
                                    ))
                                    .h(px(34.))
                                    .px(px(10.))
                                    .gap(px(6.))
                                    .tooltip("发现 GitHub 新版本，点击前往设置页更新")
                                    .child(dot(colors.success))
                                    .child(
                                        div()
                                            .text_size(px(12.5))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(rgb(colors.success))
                                            .child(if compact {
                                                "更新".to_owned()
                                            } else {
                                                format!("新版本 {label}")
                                            }),
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.change_page(Page::Settings, cx)
                                    })),
                            )
                        },
                    )
                    .child(if !compact {
                        button("engine-status", cx)
                            .h(px(34.))
                            .px(px(11.))
                            .gap(px(7.))
                            .tooltip("打开 Hashcat 设置")
                            .child(dot(engine_color))
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .text_color(rgb(colors.ink))
                                    .child(engine),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.change_page(Page::Settings, cx)
                            }))
                    } else {
                        button("engine-status", cx)
                            .w(px(32.))
                            .h(px(34.))
                            .p_0()
                            .tooltip(engine)
                            .child(dot(engine_color))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.change_page(Page::Settings, cx)
                            }))
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

    fn page_heading(&self, cx: &mut Context<Self>) -> Div {
        let colors = colors(cx);
        let (title, subtitle) = match self.page {
            Page::Task => ("密码恢复", "导入加密文件或已有哈希，配置字典、掩码或组合策略"),
            Page::Results => ("恢复结果", "搜索、预览与复制本机保存的恢复记录"),
            Page::Tools => (
                "引擎工具",
                "检索 Hashcat 支持的算法、模式与命令选项，运行自定义参数",
            ),
            Page::Forensics => ("磁盘取证", "只读分析证据镜像中的加密卷，使用已知凭据解密导出"),
            Page::Settings => ("设置", "管理 Hashcat 引擎、结果目录、内置资源与界面外观"),
        };
        let trailing: Option<Div> = match self.page {
            Page::Task if self.has_task_input(cx) && !self.form_locked() => Some(
                div().flex().items_center().gap(px(6.)).child(
                    button("reset-task-form", cx)
                        .h(px(32.))
                        .px(px(10.))
                        .gap(px(5.))
                        .child(icon(IconName::Undo2, colors.muted, 14.))
                        .child("重置配置")
                        .tooltip("清空当前任务表单中的所有输入与文件")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.reset_task_form(window, cx)
                        })),
                ),
            ),
            Page::Results => Some(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .px(px(9.))
                            .py(px(3.))
                            .rounded_full()
                            .bg(rgb(colors.raised))
                            .text_size(px(12.))
                            .text_color(rgb(colors.muted))
                            .child(format!("共 {} 条", self.results.len())),
                    )
                    .child(
                        button("refresh-results", cx)
                            .h(px(32.))
                            .px(px(10.))
                            .gap(px(5.))
                            .child(icon(IconName::Redo2, colors.muted, 14.))
                            .child("刷新")
                            .tooltip("刷新结果列表 (Ctrl+R)")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.refresh_results();
                                cx.notify();
                            })),
                    )
                    .child(
                        button("results-open-dir", cx)
                            .h(px(32.))
                            .px(px(10.))
                            .gap(px(5.))
                            .child(icon(IconName::ExternalLink, colors.muted, 14.))
                            .child("打开目录")
                            .on_click(cx.listener(|this, _, _, cx| this.open_data_dir(cx))),
                    ),
            ),
            Page::Settings if self.executable.is_some() => Some(
                div().flex().items_center().gap(px(6.)).child(
                    button("redetect-engine", cx)
                        .h(px(32.))
                        .px(px(10.))
                        .gap(px(5.))
                        .child(icon(IconName::Redo2, colors.muted, 14.))
                        .child(if self.probing {
                            "检测中…"
                        } else {
                            "重新检测引擎"
                        })
                        .disabled(self.probing || self.form_locked())
                        .on_click(cx.listener(|this, _, window, cx| {
                            if let Some(exe) = this.executable.clone() {
                                this.connect(exe, false, window, cx);
                            }
                        })),
                ),
            ),
            _ => None,
        };
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(16.))
            .flex_shrink_0()
            .px(px(24.))
            .pt(px(14.))
            .pb(px(12.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_size(px(22.))
                            .line_height(px(28.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .line_height(px(18.))
                            .text_color(rgb(colors.muted))
                            .child(subtitle),
                    ),
            )
            .when_some(trailing, |row, actions| row.child(actions.flex_shrink_0()))
    }

    fn task_page(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = colors(cx);
        let stacked =
            window.viewport_size().width < px(940.) || window.viewport_size().height < px(580.);
        div()
            .id("task-page")
            .flex()
            .flex_1()
            .min_h_0()
            .gap(px(16.))
            .px(px(24.))
            .pb(px(20.))
            .when(stacked, |this| this.flex_col().overflow_y_scroll())
            .child(self.task_form(stacked, cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .when(!stacked, |this| {
                        this.w(relative(0.38))
                            .min_w(px(330.))
                            .max_w(px(480.))
                            .h_full()
                    })
                    .when(stacked, |this| this.w_full().min_h(px(340.)).flex_shrink_0())
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
                .h(px(42.))
                .px(px(20.))
                .gap(px(8.))
                .custom(variant(
                    cx,
                    colors.error_fill,
                    colors.error,
                    colors.error,
                    colors.error_fill,
                ))
                .child(div().size(px(9.)).rounded(px(2.)).bg(rgb(colors.error)))
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(colors.error))
                        .child(if self.phase == Phase::Stopping {
                            "正在停止"
                        } else {
                            "停止任务"
                        }),
                )
                .disabled(self.phase == Phase::Stopping)
                .on_click(cx.listener(|this, _, _, cx| this.stop_task(cx)))
        } else {
            let label = if self.installation.is_some() || self.probing {
                "开始恢复"
            } else {
                "连接 Hashcat"
            };
            primary("start", cx)
                .h(px(42.))
                .px(px(22.))
                .gap(px(8.))
                .tooltip("启动恢复任务 (Ctrl+Enter)")
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
        let form_cards = div()
            .id("form-body")
            .flex()
            .flex_col()
            .when(!stacked, |this| {
                this.size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.form_scroll)
                    .pr(px(4.))
            })
            .min_h_0()
            .gap(px(14.))
            .pb(px(4.))
            .child(
                panel(colors)
                    .flex_shrink_0()
                    .p(px(18.))
                    .child(self.source_fields(cx)),
            )
            .child(self.strategy_panel(cx));
        let form_body = if stacked {
            form_cards.into_any_element()
        } else {
            div()
                .relative()
                .flex_1()
                .min_h_0()
                .child(form_cards)
                .vertical_scrollbar(&self.form_scroll)
                .into_any_element()
        };
        div()
            .flex()
            .flex_col()
            .when(!stacked, |this| this.flex_1().h_full())
            .when(stacked, |this| this.flex_shrink_0())
            .min_w_0()
            .min_h_0()
            .gap(px(14.))
            .child(form_body)
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
                        panel(colors)
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .gap(px(12.))
                            .px(px(16.))
                            .py(px(10.))
                            .child(
                                button("footer-open-results", cx)
                                    .custom(variant(
                                        cx,
                                        colors.panel,
                                        colors.muted,
                                        colors.panel,
                                        colors.raised,
                                    ))
                                    .h(px(34.))
                                    .px(px(10.))
                                    .gap(px(7.))
                                    .tooltip("点击查看本机恢复结果")
                                    .child(icon(IconName::FolderClosed, colors.muted, 15.))
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .text_color(rgb(colors.muted))
                                            .child("恢复结果保存在本机"),
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.change_page(Page::Results, cx)
                                    })),
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
                .child(section_title("输入目标"))
                .child(
                    div()
                        .flex()
                        .p(px(3.))
                        .rounded(px(9.))
                        .bg(rgb(colors.background))
                        .border_1()
                        .border_color(rgb(colors.subtle_border))
                        .gap(px(3.))
                        .children(choices.into_iter().map(|(id, label, source)| {
                            choice(id, label, self.source == source, cx)
                                .h(px(28.))
                                .px(px(11.))
                                .disabled(locked || self.picking)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.source = source;
                                    this.type_notice = None;
                                    this.form_error = None;
                                    cx.notify();
                                }))
                        })),
                ),
        );

        if self.source == Source::Protected {
            let message: AnyElement = match &self.extraction {
                Extraction::Idle => div()
                    .text_size(px(12.))
                    .text_color(rgb(colors.muted))
                    .child("支持 ZIP、RAR、7z、PDF、DOCX、XLSX、PPTX，可直接拖拽或点击上方区域选择")
                    .into_any_element(),
                Extraction::Pending => div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(12.))
                    .text_color(rgb(colors.muted))
                    .child(icon(IconName::LoaderCircle, colors.muted, 14.))
                    .child("正在读取并提取加密校验信息…")
                    .into_any_element(),
                Extraction::Ready(data) => {
                    let hash = data.hash.clone();
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(8.))
                        .px(px(10.))
                        .py(px(6.))
                        .rounded(px(8.))
                        .bg(rgb(colors.success_fill))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(7.))
                                .min_w_0()
                                .flex_1()
                                .text_size(px(12.5))
                                .text_color(rgb(colors.success))
                                .child(icon(IconName::CircleCheck, colors.success, 14.))
                                .child(
                                    div()
                                        .truncate()
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(format!(
                                            "{}  ·  Hashcat 模式 -m {}",
                                            data.format, data.mode
                                        )),
                                ),
                        )
                        .child(
                            button("copy-extracted-hash", cx)
                                .h(px(26.))
                                .px(px(8.))
                                .gap(px(4.))
                                .child(icon(
                                    if self.copied_hash {
                                        IconName::Check
                                    } else {
                                        IconName::Copy
                                    },
                                    if self.copied_hash {
                                        colors.success
                                    } else {
                                        colors.muted
                                    },
                                    12.,
                                ))
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .child(if self.copied_hash {
                                            "已复制"
                                        } else {
                                            "复制哈希"
                                        }),
                                )
                                .tooltip("复制提取出的 Hashcat 格式哈希")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(hash.clone()));
                                    this.copied_hash = true;
                                    cx.notify();
                                })),
                        )
                        .into_any_element()
                }
                Extraction::Failed(error) => {
                    error_line(colors, error.clone()).into_any_element()
                }
            };
            return group
                .child(self.file_field(
                    "protected-file",
                    self.protected_file.as_deref(),
                    FileSlot::Protected,
                    "点击选择或拖入加密文件",
                    cx,
                ))
                .child(message);
        }

        group = match self.source {
            Source::HashFile => group.child(self.file_field(
                "hash-file",
                self.hashes.as_deref(),
                FileSlot::Hashes,
                "点击选择或拖入包含哈希的文件",
                cx,
            )),
            Source::Paste => {
                let raw = self.hash_text.read(cx).value();
                let count = raw.lines().filter(|line| !line.trim().is_empty()).count();
                group
                    .child(
                        multiline(&self.hash_text, 96.)
                            .font_family("Cascadia Mono")
                            .disabled(locked),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(rgb(colors.muted))
                                    .child(if count == 0 {
                                        "每行一条哈希，支持批量粘贴".to_owned()
                                    } else {
                                        format!("已输入 {count} 条哈希")
                                    }),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .child(
                                        button("paste-clipboard", cx)
                                            .h(px(26.))
                                            .px(px(8.))
                                            .gap(px(4.))
                                            .child(icon(IconName::Copy, colors.muted, 12.))
                                            .child(div().text_size(px(12.)).child("从剪贴板粘贴"))
                                            .disabled(locked)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.paste_hashes_from_clipboard(window, cx)
                                            })),
                                    )
                                    .when(count > 0, |row| {
                                        row.child(
                                            button("clear-pasted-hashes", cx)
                                                .h(px(26.))
                                                .px(px(8.))
                                                .label("清空")
                                                .disabled(locked)
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.hash_text.update(cx, |s, cx| {
                                                        s.set_value("", window, cx)
                                                    });
                                                    this.type_notice = None;
                                                    cx.notify();
                                                })),
                                        )
                                    }),
                            ),
                    )
            }
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
                                .px(px(12.))
                                .gap(px(6.))
                                .child(icon(IconName::Search, colors.muted, 14.))
                                .child(if self.identifying {
                                    "识别中…"
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
                        .h(px(26.))
                        .gap(px(7.))
                        .tooltip("仅用于 用户名:哈希 格式（如 NTLM）；NetNTLMv2/Kerberos 等自带用户名的格式请勿勾选")
                        .disabled(locked)
                        .child(username_box)
                        .child(
                            div()
                                .text_size(px(12.5))
                                .text_color(rgb(colors.muted))
                                .child("输入含额外用户名前缀 (--username)"),
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.username = !this.username;
                            this.type_notice = None;
                            cx.notify();
                        })),
                )
                .when_some(self.has_username_conflict(cx), |this, warning| {
                    this.child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.))
                            .px(px(10.))
                            .py(px(7.))
                            .rounded(px(8.))
                            .bg(rgb(colors.error_fill))
                            .border_1()
                            .border_color(rgb(colors.border))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .min_w_0()
                                    .flex_1()
                                    .text_size(px(12.))
                                    .line_height(px(17.))
                                    .text_color(rgb(colors.error))
                                    .child(icon(IconName::TriangleAlert, colors.error, 14.))
                                    .child(warning),
                            )
                            .child(
                                button("uncheck-username", cx)
                                    .h(px(26.))
                                    .px(px(9.))
                                    .flex_shrink_0()
                                    .label("取消勾选")
                                    .disabled(locked)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.username = false;
                                        this.type_notice = None;
                                        cx.notify();
                                    })),
                            ),
                    )
                }),
        )
    }

    fn strategy_panel(&self, cx: &mut Context<Self>) -> Div {
        let colors = colors(cx);
        let locked = self.form_locked() || self.installation.is_none();
        let quick_modes: [(u32, &str, &str, &str); 5] = [
            (0, "字典", "-a 0", "词表 + 规则"),
            (3, "掩码", "-a 3", "按位枚举"),
            (1, "双字典", "-a 1", "两词拼接"),
            (6, "词 + 掩", "-a 6", "词尾追加"),
            (7, "掩 + 词", "-a 7", "词首前置"),
        ];
        let is_advanced = !quick_modes.iter().any(|(id, _, _, _)| *id == self.attack);
        let mode_hint = match self.attack {
            0 => "使用密码字典逐条尝试，可搭配 .rule 规则进行大小写和后缀变形",
            1 => "将两份字典中的词条按顺序两两拼接作为候选密码",
            3 => "按自定义字符集占位符（如 ?d、?l）定向穷举指定结构",
            4 => "基于概率上下文无关文法（PCFG）生成高概率密码结构",
            5 => "将字典词条按自定义字符替换表（.table）批量变换",
            6 => "在字典词条末尾追加掩码组合（例如 词+年份/数字）",
            7 => "在字典词条开头前置掩码组合（例如 数字+词）",
            8 => "通过 Hashcat 8 通用 Feed 模块动态提供候选密码",
            9 => "将哈希与候选字典按行逐一对应关联验证",
            12 => "在掩码中使用 ?w / ?q 占位符自由插入字典词条",
            _ => "按所选高级模式要求的顺序配置参数",
        };
        let has_quick_helper = matches!(self.attack, 0 | 1 | 3 | 6 | 7 | 12);

        panel(colors)
            .flex_shrink_0()
            .p(px(18.))
            .gap(px(12.))
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
                            .child(section_title("恢复方式"))
                            .when(is_advanced, |row| {
                                row.child(
                                    div()
                                        .px(px(7.))
                                        .py(px(2.))
                                        .rounded_full()
                                        .bg(rgb(colors.selection))
                                        .border_1()
                                        .border_color(rgb(colors.selection_border))
                                        .font_family("Cascadia Mono")
                                        .text_size(px(11.5))
                                        .text_color(rgb(colors.ink))
                                        .child(format!("高级 -a {}", self.attack)),
                                )
                            }),
                    )
                    .child(
                        div().w(px(196.)).child(
                            Select::new(&self.attacks)
                                .w_full()
                                .h(px(34.))
                                .text_size(px(13.))
                                .line_height(px(18.))
                                .placeholder("全部 / 更多模式")
                                .search_placeholder("搜索全部模式或编号")
                                .disabled(locked),
                        ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .children(quick_modes.into_iter().map(|(id, title, code, desc)| {
                        let selected = self.attack == id;
                        div()
                            .id(("mode-card", id))
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(94.))
                            .px(px(10.))
                            .py(px(8.))
                            .gap(px(2.))
                            .rounded(px(9.))
                            .border_1()
                            .border_color(rgb(if selected {
                                colors.selection_border
                            } else {
                                colors.border
                            }))
                            .bg(rgb(if selected {
                                colors.selection
                            } else {
                                colors.monitor
                            }))
                            .when(!locked, |card| {
                                card.cursor_pointer()
                                    .hover(|s| {
                                        s.bg(rgb(if selected {
                                            colors.selection_hover
                                        } else {
                                            colors.raised
                                        }))
                                        .border_color(rgb(colors.selection_border))
                                    })
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.activate_attack(id, window, cx)
                                    }))
                            })
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(4.))
                                    .child(
                                        div()
                                            .text_size(px(13.5))
                                            .font_weight(if selected {
                                                FontWeight::SEMIBOLD
                                            } else {
                                                FontWeight::MEDIUM
                                            })
                                            .text_color(rgb(if selected {
                                                colors.ink
                                            } else {
                                                colors.ink
                                            }))
                                            .child(title),
                                    )
                                    .child(
                                        div()
                                            .font_family("Cascadia Mono")
                                            .text_size(px(10.5))
                                            .px(px(5.))
                                            .py(px(1.))
                                            .rounded(px(4.))
                                            .bg(rgb(if selected {
                                                colors.panel
                                            } else {
                                                colors.raised
                                            }))
                                            .text_color(rgb(colors.muted))
                                            .child(code),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(rgb(colors.muted))
                                    .truncate()
                                    .child(desc),
                            )
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .p(px(14.))
                    .rounded(px(10.))
                    .bg(rgb(colors.monitor))
                    .border_1()
                    .border_color(rgb(colors.subtle_border))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(10.))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(rgb(colors.muted))
                                    .min_w_0()
                                    .flex_1()
                                    .child(mode_hint),
                            )
                            .when(has_quick_helper, |row| {
                                row.child(self.quick_setup_toggle(cx).flex_shrink_0())
                            }),
                    )
                    .child(self.candidate_fields(cx))
                    .when(has_quick_helper, |box_| {
                        box_.when_some(self.quick_setup_body(cx), |box_, helper| box_.child(helper))
                    }),
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
                    "点击选择或拖入候选密码字典",
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
                    "点击选择或拖入第二份字典",
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
                                .px(px(10.))
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
                                    .px(px(10.))
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
                        .text_size(px(12.5))
                        .line_height(px(18.))
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
                    "点击选择或拖入 .rule 文件",
                    cx,
                ),
            ));
        }
        fields
    }

    fn mask_field(&self, cx: &mut Context<Self>) -> Div {
        let colors = colors(cx);
        let locked = self.form_locked() || self.picking;
        let has_mask_text = !self.mask_text.read(cx).value().is_empty();
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
                    div().min_w_0().flex_1().child(
                        single_line(&self.mask_text)
                            .font_family("Cascadia Mono")
                            .disabled(self.form_locked()),
                    ),
                )
                .child(
                    button("mask-browse", cx)
                        .h(px(40.))
                        .px(px(12.))
                        .gap(px(6.))
                        .child(icon(IconName::Folder, colors.muted, 15.))
                        .child("掩码文件")
                        .disabled(locked)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.pick_file(FileSlot::Mask, window, cx)
                        })),
                )
        };
        let tokens: &[(&str, &str)] = if self.attack == 12 {
            &[
                ("?w", "字典1"),
                ("?q", "字典2"),
                ("?d", "数字"),
                ("?l", "小写"),
                ("?u", "大写"),
                ("?s", "符号"),
            ]
        } else {
            &[
                ("?d", "数字"),
                ("?l", "小写"),
                ("?u", "大写"),
                ("?s", "符号"),
                ("?a", "全字符"),
            ]
        };
        field("掩码", row).child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(5.))
                .children(tokens.iter().enumerate().map(|(i, (token, desc))| {
                    let token = *token;
                    button(("mask-chip", i), cx)
                        .h(px(26.))
                        .px(px(8.))
                        .gap(px(4.))
                        .tooltip(format!("点击追加 {token}（{desc}）"))
                        .disabled(locked)
                        .child(
                            div()
                                .font_family("Cascadia Mono")
                                .text_size(px(12.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(colors.ink))
                                .child(token),
                        )
                        .child(
                            div()
                                .text_size(px(11.5))
                                .text_color(rgb(colors.muted))
                                .child(*desc),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.mask_file = None;
                            this.mask_text.update(cx, |s, cx| {
                                s.set_value(format!("{}{token}", s.value()), window, cx)
                            });
                            this.form_error = None;
                            cx.notify();
                        }))
                }))
                .when(self.mask_file.is_none() && has_mask_text, |bar| {
                    bar.child(
                        button("mask-backspace", cx)
                            .h(px(26.))
                            .px(px(8.))
                            .label("退格")
                            .tooltip("删去最后一个掩码占位符")
                            .disabled(locked)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.mask_text.update(cx, |s, cx| {
                                    let val = s.value().to_string();
                                    let next = if val.len() >= 2
                                        && val.as_bytes()[val.len() - 2] == b'?'
                                    {
                                        val[..val.len() - 2].to_owned()
                                    } else {
                                        let mut chars = val.chars();
                                        chars.next_back();
                                        chars.as_str().to_owned()
                                    };
                                    s.set_value(next, window, cx);
                                });
                                cx.notify();
                            })),
                    )
                    .child(
                        button("mask-clear", cx)
                            .h(px(26.))
                            .px(px(8.))
                            .label("清空")
                            .disabled(locked)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.mask_text
                                    .update(cx, |s, cx| s.set_value("", window, cx));
                                cx.notify();
                            })),
                    )
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
        let clearable = path.is_some()
            && matches!(
                slot,
                FileSlot::Protected
                    | FileSlot::Hashes
                    | FileSlot::Words
                    | FileSlot::SecondWords
                    | FileSlot::Rule
                    | FileSlot::Mask
            );
        let leading = icon(
            IconName::File,
            if path.is_some() {
                colors.success
            } else if featured {
                colors.ink
            } else {
                colors.muted
            },
            if featured { 18. } else { 15. },
        );
        let subtitle = path
            .and_then(|p| p.parent())
            .map(|p| p.display().to_string());
        let drop_box = div()
            .id((id, 2_u32))
            .flex()
            .items_center()
            .gap(px(10.))
            .h(px(if featured { 60. } else { 40. }))
            .flex_1()
            .min_w_0()
            .px(px(if featured { 14. } else { 12. }))
            .rounded(px(if featured { 10. } else { 8. }))
            .bg(rgb(colors.raised))
            .border_1()
            .border_color(rgb(if path.is_some() && featured {
                colors.selection_border
            } else {
                colors.border
            }))
            .when(!locked, |this| {
                this.cursor_pointer()
                    .hover(|style| {
                        style
                            .bg(rgb(colors.hover))
                            .border_color(rgb(colors.selection_border))
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.pick_file(slot, window, cx)
                    }))
            })
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
                        this.size(px(36.))
                            .rounded(px(8.))
                            .bg(rgb(colors.panel))
                            .border_1()
                            .border_color(rgb(colors.subtle_border))
                    })
                    .child(leading),
            )
            .child(if featured {
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .flex_1()
                    .gap(px(1.))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(14.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(if path.is_some() {
                                colors.ink
                            } else {
                                colors.muted
                            }))
                            .child(path.map_or_else(|| hint.into(), hashcat::file_name)),
                    )
                    .when_some(subtitle, |col, parent| {
                        col.child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_size(px(11.5))
                                .text_color(rgb(colors.muted))
                                .child(parent),
                        )
                    })
                    .into_any_element()
            } else {
                div()
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .text_size(px(13.))
                    .text_color(rgb(if path.is_some() {
                        colors.ink
                    } else {
                        colors.muted
                    }))
                    .child(path.map_or_else(|| hint.into(), hashcat::file_name))
                    .into_any_element()
            });

        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(drop_box)
            .when(clearable, |row| {
                row.child(
                    button((id, 1_u32), cx)
                        .w(px(if featured { 40. } else { 36. }))
                        .h(px(if featured { 60. } else { 40. }))
                        .p_0()
                        .flex_shrink_0()
                        .child(icon(IconName::Close, colors.muted, 15.))
                        .tooltip("清除已选文件")
                        .disabled(locked)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.clear_file_slot(slot, cx)
                        })),
                )
            })
            .child(
                button((id, 0_u32), cx)
                    .h(px(if featured { 60. } else { 40. }))
                    .flex_shrink_0()
                    .px(px(if featured { 14. } else { 12. }))
                    .gap(px(6.))
                    .child(icon(IconName::Folder, colors.muted, 15.))
                    .child(if path.is_some() {
                        "更换"
                    } else if featured {
                        "选择文件"
                    } else {
                        "浏览"
                    })
                    .when_some(path, |btn, p| btn.tooltip(p.display().to_string()))
                    .disabled(locked)
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.pick_file(slot, window, cx)),
                    ),
            )
    }

    fn run_status(&self, narrow: bool, cx: &mut Context<Self>) -> Div {
        let colors = colors(cx);
        let (status_color, status_fill) = match self.phase {
            Phase::Starting | Phase::Running | Phase::Stopping => (colors.ink, colors.selection),
            Phase::Recovered => (colors.success, colors.success_fill),
            Phase::Failed => (colors.error, colors.error_fill),
            _ => (colors.muted, colors.raised),
        };
        let progress_bar_color = match self.phase {
            Phase::Recovered => colors.success,
            Phase::Failed => colors.error,
            _ => colors.accent,
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
            .p(px(18.))
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
                            .gap(px(6.))
                            .px(px(10.))
                            .py(px(3.))
                            .rounded_full()
                            .bg(rgb(status_fill))
                            .text_size(px(12.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(status_color))
                            .child(dot(status_color))
                            .child(self.phase.label()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(7.))
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .items_center()
                            .text_size(px(12.5))
                            .text_color(rgb(colors.muted))
                            .child("进度")
                            .child(
                                div()
                                    .text_color(rgb(colors.ink))
                                    .font_weight(FontWeight::MEDIUM)
                                    .font_family("Cascadia Mono")
                                    .child(percent),
                            ),
                    )
                    .child(
                        div()
                            .h(px(7.))
                            .w_full()
                            .rounded_full()
                            .bg(rgb(colors.raised))
                            .child(
                                div()
                                    .h_full()
                                    .w(relative(share.unwrap_or(0.) as f32))
                                    .rounded_full()
                                    .bg(rgb(progress_bar_color)),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .min_w_0()
                    .p(px(10.))
                    .rounded(px(9.))
                    .bg(rgb(colors.panel))
                    .border_1()
                    .border_color(rgb(colors.subtle_border))
                    .when(narrow, |this| this.flex_col().gap(px(6.)))
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
                let can_retry_without_username = self.phase == Phase::Failed
                    && self.cancel.is_none()
                    && self.username
                    && self.source != Source::Protected
                    && (error.contains("用户名") || error.contains("Token length"));
                this.child(error_line(colors, error.clone()))
                    .when(can_retry_without_username, |this| {
                        this.child(
                            button("retry-no-username", cx)
                                .h(px(32.))
                                .w_full()
                                .gap(px(6.))
                                .custom(variant(
                                    cx,
                                    colors.selection,
                                    colors.ink,
                                    colors.selection_border,
                                    colors.selection_hover,
                                ))
                                .child(icon(IconName::Redo2, colors.ink, 14.))
                                .child("取消勾选「输入含用户名」并重新运行")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.username = false;
                                    this.type_notice = None;
                                    this.start_task(window, cx);
                                })),
                        )
                    })
            })
            .when_some(
                self.current_result.as_ref().filter(|_| {
                    self.cancel.is_none()
                        && (!self.found_passwords.is_empty()
                            || self
                                .status
                                .recovered_hashes
                                .is_some_and(|[count, _]| count > 0))
                }),
                |this, path| {
                    let path = path.clone();
                    this.child(
                        button("show-current-result", cx)
                            .h(px(34.))
                            .w_full()
                            .gap(px(6.))
                            .custom(variant(
                                cx,
                                colors.panel,
                                colors.ink,
                                colors.border,
                                colors.selection,
                            ))
                            .child(icon(IconName::FolderOpen, colors.success, 15.))
                            .child(div().text_color(rgb(colors.ink)).child("查看结果文件"))
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
        let count = self.found_passwords.len();
        div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .gap(px(8.))
            .px(px(18.))
            .py(px(12.))
            .bg(rgb(c.selection))
            .border_b_1()
            .border_color(rgb(c.border))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(icon(IconName::CircleCheck, c.success, 15.))
                            .child(section_title("已恢复密码"))
                            .child(
                                div()
                                    .px(px(7.))
                                    .py(px(1.))
                                    .rounded_full()
                                    .bg(rgb(c.panel))
                                    .text_size(px(11.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(c.ink))
                                    .child(format!("{count}")),
                            ),
                    )
                    .child(
                        button("copy-passwords", cx)
                            .h(px(28.))
                            .px(px(9.))
                            .gap(px(4.))
                            .child(icon(
                                if self.copied_passwords {
                                    IconName::Check
                                } else {
                                    IconName::Copy
                                },
                                if self.copied_passwords {
                                    c.success
                                } else {
                                    c.muted
                                },
                                13.,
                            ))
                            .child(if self.copied_passwords {
                                "已复制"
                            } else {
                                "复制全部"
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(
                                    this.found_passwords
                                        .iter()
                                        .rev()
                                        .map(String::as_str)
                                        .collect::<Vec<_>>()
                                        .join("\n"),
                                ));
                                this.copied_passwords = true;
                                this.copied_password_idx = None;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                gpui::uniform_list(
                    "inline-passwords",
                    count,
                    cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                        let c = colors(cx);
                        range
                            .filter_map(|i| {
                                let v = this.found_passwords.iter().rev().nth(i)?;
                                let copied = this.copied_password_idx == Some(i);
                                let raw = v.clone();
                                let display = if v.is_empty() {
                                    "（空密码）".to_owned()
                                } else {
                                    v.clone()
                                };
                                Some(
                                    div()
                                        .id(("found-pw-row", i))
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .gap(px(8.))
                                        .h(px(28.))
                                        .px(px(8.))
                                        .rounded(px(6.))
                                        .cursor_pointer()
                                        .hover(|s| s.bg(rgb(c.panel)))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                raw.clone(),
                                            ));
                                            this.copied_password_idx = Some(i);
                                            this.copied_passwords = false;
                                            cx.notify();
                                        }))
                                        .child(
                                            div()
                                                .min_w_0()
                                                .flex_1()
                                                .truncate()
                                                .font_family("Cascadia Mono")
                                                .text_size(px(14.))
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(display),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap(px(4.))
                                                .flex_shrink_0()
                                                .text_size(px(11.5))
                                                .text_color(rgb(if copied {
                                                    c.success
                                                } else {
                                                    c.muted
                                                }))
                                                .child(icon(
                                                    if copied {
                                                        IconName::Check
                                                    } else {
                                                        IconName::Copy
                                                    },
                                                    if copied { c.success } else { c.muted },
                                                    12.,
                                                ))
                                                .when(copied, |el| el.child("已复制")),
                                        ),
                                )
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .h(px(28. * count.min(5) as f32)),
            )
    }

    fn log_panel(&self, cx: &mut Context<Self>) -> Div {
        let colors = colors(cx);
        let copy_color = if self.copied_log {
            colors.success
        } else {
            colors.muted
        };
        let has_logs = !self.logs.is_empty();
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .h(px(44.))
                    .px(px(18.))
                    .flex_shrink_0()
                    .border_b_1()
                    .border_color(rgb(colors.subtle_border))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(7.))
                            .text_color(rgb(colors.ink))
                            .text_size(px(13.5))
                            .font_weight(FontWeight::MEDIUM)
                            .child(icon(IconName::SquareTerminal, colors.muted, 15.))
                            .child("运行日志")
                            .when(has_logs, |row| {
                                row.child(
                                    div()
                                        .px(px(6.))
                                        .py(px(1.))
                                        .rounded_full()
                                        .bg(rgb(colors.raised))
                                        .text_size(px(11.))
                                        .text_color(rgb(colors.muted))
                                        .child(format!("{} 行", self.logs.len())),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .when(has_logs, |row| {
                                row.child(
                                    button("scroll-log-bottom", cx)
                                        .h(px(26.))
                                        .w(px(26.))
                                        .p_0()
                                        .custom(variant(
                                            cx,
                                            colors.monitor,
                                            colors.muted,
                                            colors.monitor,
                                            colors.raised,
                                        ))
                                        .tooltip("滚动到底部")
                                        .child(icon(IconName::ArrowDown, colors.muted, 13.))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.log_scroll.scroll_to_bottom();
                                            cx.notify();
                                        })),
                                )
                            })
                            .when(has_logs && self.cancel.is_none(), |row| {
                                row.child(
                                    button("clear-log", cx)
                                        .h(px(26.))
                                        .px(px(7.))
                                        .custom(variant(
                                            cx,
                                            colors.monitor,
                                            colors.muted,
                                            colors.monitor,
                                            colors.raised,
                                        ))
                                        .tooltip("清空日志并重置状态")
                                        .child(
                                            div()
                                                .text_size(px(12.))
                                                .text_color(rgb(colors.muted))
                                                .child("清空"),
                                        )
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.clear_logs(cx)
                                        })),
                                )
                            })
                            .child(
                                button("copy-log", cx)
                                    .h(px(26.))
                                    .px(px(8.))
                                    .gap(px(4.))
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
                                        13.,
                                    ))
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(rgb(copy_color))
                                            .child(if self.copied_log { "已复制" } else { "复制" }),
                                    )
                                    .disabled(!has_logs)
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
                    ),
            )
            .child(if !has_logs {
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap(px(8.))
                    .px(px(20.))
                    .py(px(24.))
                    .text_color(rgb(colors.muted))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(px(40.))
                            .rounded(px(10.))
                            .bg(rgb(colors.raised))
                            .child(icon(IconName::SquareTerminal, colors.muted, 18.)),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(if self.cancel.is_some() {
                                "正在等待 Hashcat 输出…"
                            } else {
                                "启动任务后实时显示运行日志"
                            }),
                    )
                    .when(self.cancel.is_none(), |box_| {
                        box_.child(
                            div()
                                .text_size(px(12.))
                                .text_color(rgb(colors.muted))
                                .child("支持按 Ctrl+Enter 快速启动"),
                        )
                    })
                    .into_any_element()
            } else {
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(
                        div()
                            .id("log-scroller")
                            .flex()
                            .flex_col()
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.log_scroll)
                            .px(px(18.))
                            .py(px(12.))
                            .gap(px(4.))
                            .font_family("Cascadia Mono")
                            .text_size(px(12.5))
                            .line_height(px(19.))
                            .children(self.logs.iter().cloned().map(|line| {
                                let lower = line.to_lowercase();
                                let line_color = if lower.contains("error")
                                    || lower.contains("exception")
                                    || lower.contains("no hashes loaded")
                                {
                                    colors.error
                                } else if line.starts_with("* Device")
                                    || line.starts_with("Initialized")
                                    || line.starts_with("Status")
                                    || line.starts_with("Recovered")
                                {
                                    colors.ink
                                } else {
                                    colors.muted
                                };
                                div()
                                    .min_w_0()
                                    .flex_shrink_0()
                                    .text_color(rgb(line_color))
                                    .child(line)
                            })),
                    )
                    .vertical_scrollbar(&self.log_scroll)
                    .into_any_element()
            })
    }

    fn results_page(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let colors = colors(cx);
        let stacked = window.viewport_size().width < px(820.);
        let query = self.results_search.read(cx).value().trim().to_lowercase();
        let filtered: Vec<&hashcat::ResultFile> = self
            .results
            .iter()
            .filter(|file| {
                query.is_empty()
                    || file.source.to_lowercase().contains(&query)
                    || file
                        .path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.to_lowercase().contains(&query))
            })
            .collect();
        let active_meta = self
            .selected_result
            .as_ref()
            .and_then(|path| self.results.iter().find(|f| &f.path == path));

        let files = panel(colors)
            .when(!stacked, |p| p.w(px(284.)).flex_shrink_0())
            .when(stacked, |p| p.w_full().h(px(240.)).flex_shrink_0())
            .min_h_0()
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .p(px(14.))
                    .flex_shrink_0()
                    .border_b_1()
                    .border_color(rgb(colors.subtle_border))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(7.))
                                    .child(icon(IconName::FolderOpen, colors.success, 15.))
                                    .child(section_title("本地记录")),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(rgb(colors.muted))
                                    .child(format!("{} / {}", filtered.len(), self.results.len())),
                            ),
                    )
                    .child(
                        single_line(&self.results_search)
                            .prefix(icon(IconName::Search, colors.muted, 15.))
                            .cleanable(true),
                    ),
            )
            .child(
                div()
                    .id("results-list")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(px(10.))
                    .gap(px(4.))
                    .when(filtered.is_empty(), |this| {
                        this.child(
                            div()
                                .text_size(px(13.))
                                .text_color(rgb(colors.muted))
                                .px(px(10.))
                                .py(px(12.))
                                .child(if self.results.is_empty() {
                                    "暂无已恢复的结果"
                                } else {
                                    "没有匹配的记录"
                                }),
                        )
                    })
                    .children(filtered.into_iter().enumerate().map(|(index, file)| {
                        let path = file.path.clone();
                        let time: DateTime<Local> = file.modified.into();
                        let selected = self.selected_result.as_ref() == Some(&path);
                        let (fill, border, hover) = if selected {
                            (
                                colors.selection,
                                colors.selection_border,
                                colors.selection_hover,
                            )
                        } else {
                            (colors.panel, colors.panel, colors.raised)
                        };
                        button(("result-row", index), cx)
                            .custom(variant(cx, fill, colors.ink, border, hover))
                            .h(px(56.))
                            .w_full()
                            .justify_start()
                            .px(px(10.))
                            .gap(px(9.))
                            .child(icon(IconName::File, colors.success, 15.))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_w_0()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .truncate()
                                            .font_weight(if selected {
                                                FontWeight::SEMIBOLD
                                            } else {
                                                FontWeight::MEDIUM
                                            })
                                            .child(file.source.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.5))
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
        let preview_title = active_meta
            .map(|m| m.source.clone())
            .unwrap_or_else(|| "结果预览".to_owned());
        let copy_content = match &self.result_preview {
            Some(Ok((content, _))) => Some(content.clone()),
            _ => None,
        };
        let extracted_passwords: Option<String> = match &self.result_preview {
            Some(Ok((content, _))) => {
                let mut any_hash_prefix = false;
                let mut list = Vec::new();
                for line in content.lines().filter(|l| !l.trim().is_empty()) {
                    let (hash, pass) = hashcat::split_result_line(line);
                    if hash.is_some() {
                        any_hash_prefix = true;
                    }
                    list.push(pass);
                }
                if any_hash_prefix && !list.is_empty() {
                    Some(list.join("\n"))
                } else {
                    None
                }
            }
            _ => None,
        };
        let display = panel(colors)
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(10.))
                    .px(px(18.))
                    .h(px(50.))
                    .flex_shrink_0()
                    .border_b_1()
                    .border_color(rgb(colors.subtle_border))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(px(15.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(preview_title),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .flex_shrink_0()
                            .when(copy_content.is_some(), |row| {
                                row.child(
                                    choice("toggle-result-wrap", "自动换行", self.result_wrap, cx)
                                        .h(px(30.))
                                        .px(px(10.))
                                        .tooltip(if self.result_wrap {
                                            "当前自动换行，点击切换为单行横向滚动"
                                        } else {
                                            "当前单行横向滚动，点击开启自动换行"
                                        })
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.result_wrap = !this.result_wrap;
                                            this.results_scroll
                                                .set_offset(gpui::point(px(0.), px(0.)));
                                            cx.notify();
                                        })),
                                )
                            })
                            .when_some(extracted_passwords, |row, passwords| {
                                row.child(
                                    button("copy-result-passwords", cx)
                                        .h(px(30.))
                                        .px(px(10.))
                                        .gap(px(5.))
                                        .child(icon(
                                            if self.copied_result_passwords {
                                                IconName::Check
                                            } else {
                                                IconName::Copy
                                            },
                                            if self.copied_result_passwords {
                                                colors.success
                                            } else {
                                                colors.success
                                            },
                                            13.,
                                        ))
                                        .child(if self.copied_result_passwords {
                                            "已复制密码"
                                        } else {
                                            "复制密码"
                                        })
                                        .tooltip("仅复制解析出的明文密码（不含哈希前缀）")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                passwords.clone(),
                                            ));
                                            this.copied_result_passwords = true;
                                            this.copied_result = false;
                                            this.copied_result_row = None;
                                            cx.notify();
                                        })),
                                )
                            })
                            .when_some(copy_content, |row, content| {
                                row.child(
                                    button("copy-result-preview", cx)
                                        .h(px(30.))
                                        .px(px(10.))
                                        .gap(px(5.))
                                        .child(icon(
                                            if self.copied_result {
                                                IconName::Check
                                            } else {
                                                IconName::Copy
                                            },
                                            if self.copied_result {
                                                colors.success
                                            } else {
                                                colors.muted
                                            },
                                            13.,
                                        ))
                                        .child(if self.copied_result {
                                            "已复制"
                                        } else {
                                            "复制全文"
                                        })
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                content.clone(),
                                            ));
                                            this.copied_result = true;
                                            this.copied_result_passwords = false;
                                            this.copied_result_row = None;
                                            cx.notify();
                                        })),
                                )
                            })
                            .when_some(self.selected_result.as_ref(), |row, path| {
                                let path = path.clone();
                                row.child(
                                    button("reveal-result", cx)
                                        .h(px(30.))
                                        .px(px(10.))
                                        .gap(px(5.))
                                        .child(icon(IconName::ExternalLink, colors.muted, 13.))
                                        .child("定位文件")
                                        .tooltip("在资源管理器中选中此结果文件")
                                        .on_click(move |_, _, cx| cx.reveal_path(&path)),
                                )
                            }),
                    ),
            )
            .child(match &self.result_preview {
                Some(Ok((content, truncated))) => {
                    let non_empty: Vec<&str> =
                        content.lines().filter(|l| !l.trim().is_empty()).collect();
                    let lines = content.lines().count();
                    let body: AnyElement = if self.result_wrap {
                        let inner: AnyElement = if non_empty.len() <= 200 {
                            div()
                                .flex()
                                .flex_col()
                                .w_full()
                                .min_w_0()
                                .gap(px(10.))
                                .children(non_empty.into_iter().enumerate().map(|(i, raw)| {
                                    let (hash_prefix, password) = hashcat::split_result_line(raw);
                                    let copied = self.copied_result_row == Some(i);
                                    let copy_val = password.clone();
                                    let display_pw = if password.is_empty() {
                                        "（空密码）".to_owned()
                                    } else {
                                        password
                                    };
                                    div()
                                        .flex()
                                        .flex_col()
                                        .w_full()
                                        .min_w_0()
                                        .gap(px(6.))
                                        .p(px(12.))
                                        .rounded(px(9.))
                                        .bg(rgb(colors.panel))
                                        .border_1()
                                        .border_color(rgb(colors.subtle_border))
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .justify_between()
                                                .flex_wrap()
                                                .gap(px(8.))
                                                .child(
                                                    div()
                                                        .flex()
                                                        .items_center()
                                                        .min_w_0()
                                                        .flex_1()
                                                        .gap(px(8.))
                                                        .child(
                                                            div()
                                                                .flex_shrink_0()
                                                                .px(px(6.))
                                                                .py(px(1.))
                                                                .rounded(px(5.))
                                                                .bg(rgb(colors.raised))
                                                                .font_family("Cascadia Mono")
                                                                .text_size(px(11.))
                                                                .text_color(rgb(colors.muted))
                                                                .child(format!("#{}", i + 1)),
                                                        )
                                                        .child(
                                                            div()
                                                                .flex()
                                                                .items_center()
                                                                .min_w_0()
                                                                .gap(px(6.))
                                                                .px(px(9.))
                                                                .py(px(3.))
                                                                .rounded(px(6.))
                                                                .bg(rgb(colors.success_fill))
                                                                .child(
                                                                    div()
                                                                        .flex_shrink_0()
                                                                        .text_size(px(11.5))
                                                                        .text_color(rgb(
                                                                            colors.success,
                                                                        ))
                                                                        .child("明文"),
                                                                )
                                                                .child(
                                                                    div()
                                                                        .min_w_0()
                                                                        .font_family(
                                                                            "Cascadia Mono",
                                                                        )
                                                                        .text_size(px(13.5))
                                                                        .font_weight(
                                                                            FontWeight::SEMIBOLD,
                                                                        )
                                                                        .text_color(rgb(colors.ink))
                                                                        .child(display_pw),
                                                                ),
                                                        ),
                                                )
                                                .child(
                                                    button(("copy-result-row", i), cx)
                                                        .h(px(26.))
                                                        .px(px(8.))
                                                        .gap(px(4.))
                                                        .flex_shrink_0()
                                                        .child(icon(
                                                            if copied {
                                                                IconName::Check
                                                            } else {
                                                                IconName::Copy
                                                            },
                                                            if copied {
                                                                colors.success
                                                            } else {
                                                                colors.muted
                                                            },
                                                            12.,
                                                        ))
                                                        .child(
                                                            div().text_size(px(12.)).child(
                                                                if copied {
                                                                    "已复制"
                                                                } else {
                                                                    "复制密码"
                                                                },
                                                            ),
                                                        )
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                cx.write_to_clipboard(
                                                                    ClipboardItem::new_string(
                                                                        copy_val.clone(),
                                                                    ),
                                                                );
                                                                this.copied_result_row = Some(i);
                                                                this.copied_result = false;
                                                                this.copied_result_passwords =
                                                                    false;
                                                                cx.notify();
                                                            },
                                                        )),
                                                ),
                                        )
                                        .when(hash_prefix.is_some(), |card| {
                                            card.child(
                                                div()
                                                    .w_full()
                                                    .min_w_0()
                                                    .pt(px(4.))
                                                    .border_t_1()
                                                    .border_color(rgb(colors.subtle_border))
                                                    .font_family("Cascadia Mono")
                                                    .text_size(px(12.5))
                                                    .line_height(px(20.))
                                                    .text_color(rgb(colors.muted))
                                                    .child(raw.to_owned()),
                                            )
                                        })
                                }))
                                .into_any_element()
                        } else {
                            div()
                                .w_full()
                                .min_w_0()
                                .font_family("Cascadia Mono")
                                .text_size(px(13.))
                                .line_height(px(21.))
                                .child(content.clone())
                                .into_any_element()
                        };
                        div()
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .child(
                                div()
                                    .id("result-preview")
                                    .size_full()
                                    .min_w_0()
                                    .overflow_y_scroll()
                                    .track_scroll(&self.results_scroll)
                                    .p(px(16.))
                                    .bg(rgb(colors.monitor))
                                    .child(inner),
                            )
                            .vertical_scrollbar(&self.results_scroll)
                            .into_any_element()
                    } else {
                        div()
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .child(
                                div()
                                    .id("result-preview")
                                    .size_full()
                                    .overflow_scroll()
                                    .track_scroll(&self.results_scroll)
                                    .p(px(18.))
                                    .bg(rgb(colors.monitor))
                                    .font_family("Cascadia Mono")
                                    .text_size(px(13.))
                                    .line_height(px(21.))
                                    .child(div().whitespace_nowrap().child(content.clone())),
                            )
                            .vertical_scrollbar(&self.results_scroll)
                            .horizontal_scrollbar(&self.results_scroll)
                            .into_any_element()
                    };
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .child(body)
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .px(px(18.))
                                .py(px(9.))
                                .border_t_1()
                                .border_color(rgb(colors.subtle_border))
                                .text_size(px(12.))
                                .text_color(rgb(colors.muted))
                                .child(if *truncated {
                                    "预览已截断（超过 64 KB），请点击「定位文件」查看完整内容。"
                                } else if self.result_wrap {
                                    "已开启自动换行，超长单行会完整折行显示；也可切换为单行横向滚动。"
                                } else {
                                    "当前为单行模式，可使用底部横向滚动条查看完整内容。"
                                })
                                .child(format!("{lines} 行")),
                        )
                        .into_any_element()
                }
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
                    .gap(px(12.))
                    .p(px(24.))
                    .text_color(rgb(colors.muted))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(px(48.))
                            .rounded(px(12.))
                            .bg(rgb(colors.raised))
                            .child(icon(IconName::File, colors.muted, 24.)),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .font_weight(FontWeight::MEDIUM)
                            .child("找到密码后，结果会自动保存在这里"),
                    )
                    .child(
                        button("goto-task-from-results", cx)
                            .h(px(34.))
                            .px(px(14.))
                            .label("前往新建恢复任务")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.change_page(Page::Task, cx)
                            })),
                    )
                    .into_any_element(),
            });
        div()
            .flex()
            .flex_1()
            .min_h_0()
            .gap(px(16.))
            .px(px(24.))
            .pb(px(20.))
            .when(stacked, |this| this.flex_col())
            .child(files)
            .child(display)
    }

    fn environment_page(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = colors(cx);
        let two_col = window.viewport_size().width >= px(920.);
        let (device_text, device_summary, has_visual) = match &self.devices {
            DeviceQuery::Ready(report) => {
                let total_devices: usize = report.backends.iter().map(|b| b.devices.len()).sum();
                let summary = if !report.backends.is_empty() {
                    Some(format!(
                        "{} 个后端 · {} 个设备",
                        report.backends.len(),
                        total_devices
                    ))
                } else {
                    None
                };
                (Some(report.raw.clone()), summary, !report.backends.is_empty())
            }
            _ => (None, None, false),
        };
        let details: AnyElement = match &self.devices {
            DeviceQuery::Idle => div()
                .flex()
                .items_center()
                .gap(px(10.))
                .p(px(14.))
                .rounded(px(9.))
                .bg(rgb(colors.monitor))
                .border_1()
                .border_color(rgb(colors.subtle_border))
                .text_size(px(13.))
                .text_color(rgb(colors.muted))
                .child(icon(IconName::Search, colors.muted, 16.))
                .child("点击右上角「检测设备」，可视化查看本机可用的 CUDA / OpenCL 显卡、显存与驱动信息。")
                .into_any_element(),
            DeviceQuery::Pending => div()
                .flex()
                .items_center()
                .gap(px(8.))
                .p(px(14.))
                .rounded(px(9.))
                .bg(rgb(colors.monitor))
                .text_size(px(13.))
                .text_color(rgb(colors.muted))
                .child(icon(IconName::LoaderCircle, colors.muted, 15.))
                .child("正在检测本机计算后端与显卡信息…")
                .into_any_element(),
            DeviceQuery::Ready(report) if !self.devices_raw && !report.backends.is_empty() => {
                self.devices_visual_cards(report, two_col, colors)
                    .into_any_element()
            }
            DeviceQuery::Ready(report) => div()
                .id("devices-raw-details")
                .max_h(px(340.))
                .overflow_y_scroll()
                .p(px(14.))
                .rounded(px(9.))
                .bg(rgb(colors.monitor))
                .border_1()
                .border_color(rgb(colors.subtle_border))
                .font_family("Cascadia Mono")
                .text_size(px(12.))
                .line_height(px(19.))
                .text_color(rgb(colors.muted))
                .child(report.raw.clone())
                .into_any_element(),
            DeviceQuery::Failed(text) => error_line(colors, text.clone()).into_any_element(),
        };
        let version = self.installation.as_ref().map(|installation| {
            format!(
                "{} · 支持 {} 种哈希模式",
                installation.version,
                installation.modes.len()
            )
        });
        let engine_card = panel(colors)
            .flex_shrink_0()
            .child(panel_header(colors, "Hashcat 引擎", IconName::Settings))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .p(px(18.))
                    .gap(px(10.))
                    .child(self.file_field(
                        "engine-location",
                        self.executable.as_deref(),
                        FileSlot::Executable,
                        if cfg!(windows) {
                            "选择 Hashcat 8 的 hashcat.exe"
                        } else {
                            "选择 Hashcat 可执行文件"
                        },
                        cx,
                    ))
                    .when_some(self.executable.as_ref(), |this, path| {
                        this.child(
                            div()
                                .truncate()
                                .font_family("Cascadia Mono")
                                .text_size(px(11.5))
                                .text_color(rgb(colors.muted))
                                .child(path.display().to_string()),
                        )
                    })
                    .when_some(version, |this, version| {
                        this.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .px(px(10.))
                                .py(px(6.))
                                .rounded(px(7.))
                                .bg(rgb(colors.success_fill))
                                .text_size(px(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(rgb(colors.success))
                                .child(icon(IconName::CircleCheck, colors.success, 14.))
                                .child(version),
                        )
                    })
                    .when_some(self.engine_notice.as_ref(), |this, note| {
                        this.child(notice(colors, note))
                    }),
            );
        let row1 = div()
            .flex()
            .flex_shrink_0()
            .gap(px(14.))
            .when(!two_col, |row| row.flex_col())
            .child(engine_card.when(two_col, |c| c.flex_1().min_w_0()))
            .child(
                self.storage_panel(cx)
                    .when(two_col, |c| c.flex_1().min_w_0()),
            );
        let row2 = div()
            .flex()
            .flex_shrink_0()
            .gap(px(14.))
            .when(!two_col, |row| row.flex_col())
            .child(
                self.appearance_panel(cx)
                    .when(two_col, |c| c.flex_1().min_w_0()),
            )
            .child(
                self.download_panel(cx)
                    .when(two_col, |c| c.flex_1().min_w_0()),
            );
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .id("settings-scroller")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.settings_scroll)
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .px(px(24.))
                    .pb(px(24.))
                    .child(row1)
                    .child(row2)
                    .child(self.update_panel(cx))
                    .child(
                        panel(colors)
                            .flex_shrink_0()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(10.))
                                    .h(px(48.))
                                    .flex_shrink_0()
                                    .px(px(18.))
                                    .border_b_1()
                                    .border_color(rgb(colors.subtle_border))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.))
                                            .child(icon(IconName::SquareTerminal, colors.success, 16.))
                                            .child(section_title("计算设备"))
                                            .when_some(device_summary, |row, summary| {
                                                row.child(
                                                    div()
                                                        .px(px(8.))
                                                        .py(px(2.))
                                                        .rounded_full()
                                                        .bg(rgb(colors.success_fill))
                                                        .text_size(px(11.5))
                                                        .font_weight(FontWeight::MEDIUM)
                                                        .text_color(rgb(colors.success))
                                                        .child(summary),
                                                )
                                            }),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(6.))
                                            .when(has_visual, |row| {
                                                row.child(
                                                    button("toggle-devices-view", cx)
                                                        .h(px(30.))
                                                        .px(px(9.))
                                                        .label(if self.devices_raw {
                                                            "卡片视图"
                                                        } else {
                                                            "原始文本"
                                                        })
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            this.devices_raw = !this.devices_raw;
                                                            cx.notify();
                                                        })),
                                                )
                                            })
                                            .when_some(device_text, |row, text| {
                                                row.child(
                                                    button("copy-devices", cx)
                                                        .h(px(30.))
                                                        .px(px(9.))
                                                        .gap(px(5.))
                                                        .child(icon(
                                                            if self.copied_devices {
                                                                IconName::Check
                                                            } else {
                                                                IconName::Copy
                                                            },
                                                            if self.copied_devices {
                                                                colors.success
                                                            } else {
                                                                colors.muted
                                                            },
                                                            13.,
                                                        ))
                                                        .child(if self.copied_devices {
                                                            "已复制"
                                                        } else {
                                                            "复制"
                                                        })
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                cx.write_to_clipboard(
                                                                    ClipboardItem::new_string(
                                                                        text.clone(),
                                                                    ),
                                                                );
                                                                this.copied_devices = true;
                                                                cx.notify();
                                                            },
                                                        )),
                                                )
                                            })
                                            .child(
                                                button("refresh-devices", cx)
                                                    .h(px(30.))
                                                    .px(px(10.))
                                                    .gap(px(6.))
                                                    .child(icon(IconName::Search, colors.success, 14.))
                                                    .child("检测设备")
                                                    .disabled(
                                                        self.installation.is_none()
                                                            || self.form_locked()
                                                            || matches!(
                                                                self.devices,
                                                                DeviceQuery::Pending
                                                            ),
                                                    )
                                                    .on_click(cx.listener(
                                                        |this, _, window, cx| {
                                                            this.refresh_devices(window, cx)
                                                        },
                                                    )),
                                            ),
                                    ),
                            )
                            .child(div().p(px(18.)).child(details)),
                    ),
            )
            .vertical_scrollbar(&self.settings_scroll)
    }

    fn devices_visual_cards(
        &self,
        report: &hashcat::DeviceReport,
        two_col: bool,
        colors: Palette,
    ) -> Div {
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .children(report.backends.iter().map(|backend| {
                let is_cuda = backend.name.eq_ignore_ascii_case("CUDA");
                let badge_bg = if is_cuda {
                    colors.success_fill
                } else {
                    colors.selection
                };
                let badge_fg = if is_cuda { colors.success } else { colors.ink };
                let unit_suffix = if is_cuda { " SM" } else { " CU" };
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .flex_wrap()
                            .gap(px(8.))
                            .child(
                                div()
                                    .px(px(9.))
                                    .py(px(2.))
                                    .rounded_full()
                                    .bg(rgb(badge_bg))
                                    .border_1()
                                    .border_color(rgb(if is_cuda {
                                        colors.subtle_border
                                    } else {
                                        colors.selection_border
                                    }))
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(badge_fg))
                                    .child(backend.name.clone()),
                            )
                            .when(!backend.detail.is_empty(), |row| {
                                row.child(
                                    div()
                                        .text_size(px(12.5))
                                        .text_color(rgb(colors.muted))
                                        .child(backend.detail.clone()),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(px(12.))
                            .children(backend.devices.iter().map(|dev| {
                                let mem_bar = dev.memory_total_mb.filter(|&t| t > 0).map(|total| {
                                    let free = dev.memory_free_mb.unwrap_or(total).min(total);
                                    let free_ratio = (free as f32 / total as f32).clamp(0., 1.);
                                    let used = total.saturating_sub(free);
                                    let bar_color = if free_ratio < 0.2 {
                                        colors.error
                                    } else {
                                        colors.success
                                    };
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(6.))
                                        .p(px(10.))
                                        .rounded(px(8.))
                                        .bg(rgb(colors.panel))
                                        .border_1()
                                        .border_color(rgb(colors.subtle_border))
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .justify_between()
                                                .gap(px(8.))
                                                .text_size(px(12.))
                                                .child(
                                                    div()
                                                        .flex()
                                                        .items_center()
                                                        .gap(px(6.))
                                                        .text_color(rgb(colors.muted))
                                                        .child("显存 / 内存可用")
                                                        .when_some(
                                                            dev.memory_note.as_ref(),
                                                            |row, note| {
                                                                row.child(
                                                                    div()
                                                                        .px(px(6.))
                                                                        .py(px(1.))
                                                                        .rounded(px(4.))
                                                                        .bg(rgb(colors.raised))
                                                                        .text_size(px(11.))
                                                                        .child(note.clone()),
                                                                )
                                                            },
                                                        ),
                                                )
                                                .child(
                                                    div()
                                                        .font_family("Cascadia Mono")
                                                        .font_weight(FontWeight::MEDIUM)
                                                        .text_color(rgb(colors.ink))
                                                        .child(format!(
                                                            "{free} / {total} MB ({:.0}%)",
                                                            free_ratio * 100.
                                                        )),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .h(px(7.))
                                                .w_full()
                                                .rounded_full()
                                                .bg(rgb(colors.raised))
                                                .child(
                                                    div()
                                                        .h_full()
                                                        .w(relative(free_ratio))
                                                        .rounded_full()
                                                        .bg(rgb(bar_color)),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .justify_between()
                                                .text_size(px(11.))
                                                .text_color(rgb(colors.muted))
                                                .child(format!("已占用 {used} MB"))
                                                .child(format!("空闲可用 {free} MB")),
                                        )
                                });

                                let mut specs: Vec<(&'static str, String)> = Vec::new();
                                if let Some(p) = &dev.processors {
                                    specs.push(("计算单元", format!("{p}{unit_suffix}")));
                                }
                                if let Some(clock) = &dev.clock_mhz {
                                    specs.push(("核心频率", format!("{clock} MHz")));
                                }
                                if let Some(thrd) = &dev.preferred_threads {
                                    specs.push(("推荐线程", thrd.clone()));
                                }
                                if let Some(cache) = &dev.cache_size {
                                    specs.push(("缓存大小", cache.clone()));
                                }
                                if let Some(local) = &dev.local_memory {
                                    specs.push(("本地内存", local.clone()));
                                }
                                if let Some(drv) = &dev.driver {
                                    specs.push(("驱动版本", drv.clone()));
                                }
                                if let Some(pci) = &dev.pci {
                                    specs.push(("PCIe 地址", pci.clone()));
                                }

                                div()
                                    .flex()
                                    .flex_col()
                                    .when(two_col && backend.devices.len() > 1, |c| {
                                        c.flex_1().min_w(px(360.))
                                    })
                                    .when(!two_col || backend.devices.len() == 1, |c| c.w_full())
                                    .p(px(14.))
                                    .gap(px(10.))
                                    .rounded(px(10.))
                                    .bg(rgb(colors.monitor))
                                    .border_1()
                                    .border_color(rgb(colors.border))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .flex_wrap()
                                            .gap(px(8.))
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(8.))
                                                    .min_w_0()
                                                    .child(
                                                        div()
                                                            .px(px(7.))
                                                            .py(px(2.))
                                                            .rounded(px(5.))
                                                            .bg(rgb(colors.accent))
                                                            .text_size(px(11.))
                                                            .font_weight(FontWeight::SEMIBOLD)
                                                            .text_color(rgb(colors.accent_ink))
                                                            .child(dev.device_type.clone()),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(px(14.5))
                                                            .font_weight(FontWeight::SEMIBOLD)
                                                            .text_color(rgb(colors.ink))
                                                            .truncate()
                                                            .child(dev.name.clone()),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(6.))
                                                    .font_family("Cascadia Mono")
                                                    .text_size(px(11.5))
                                                    .child(
                                                        div()
                                                            .px(px(7.))
                                                            .py(px(2.))
                                                            .rounded_full()
                                                            .bg(rgb(colors.raised))
                                                            .text_color(rgb(colors.ink))
                                                            .child(format!("ID {}", dev.id)),
                                                    )
                                                    .when_some(dev.alias.as_ref(), |row, alias| {
                                                        row.child(
                                                            div()
                                                                .px(px(7.))
                                                                .py(px(2.))
                                                                .rounded_full()
                                                                .bg(rgb(colors.panel))
                                                                .border_1()
                                                                .border_color(rgb(
                                                                    colors.subtle_border,
                                                                ))
                                                                .text_color(rgb(colors.muted))
                                                                .child(format!("别名 {alias}")),
                                                        )
                                                    }),
                                            ),
                                    )
                                    .when_some(mem_bar, |card, bar| card.child(bar))
                                    .when(!specs.is_empty(), |card| {
                                        card.child(
                                            div().flex().flex_wrap().gap(px(8.)).children(
                                                specs.into_iter().map(|(label, val)| {
                                                    div()
                                                        .flex()
                                                        .flex_col()
                                                        .gap(px(2.))
                                                        .min_w(px(108.))
                                                        .px(px(10.))
                                                        .py(px(6.))
                                                        .rounded(px(7.))
                                                        .bg(rgb(colors.panel))
                                                        .border_1()
                                                        .border_color(rgb(colors.subtle_border))
                                                        .child(
                                                            div()
                                                                .text_size(px(11.))
                                                                .text_color(rgb(colors.muted))
                                                                .child(label),
                                                        )
                                                        .child(
                                                            div()
                                                                .font_family("Cascadia Mono")
                                                                .text_size(px(12.5))
                                                                .font_weight(FontWeight::MEDIUM)
                                                                .text_color(rgb(colors.ink))
                                                                .child(val),
                                                        )
                                                }),
                                            ),
                                        )
                                    })
                            })),
                    )
            }))
    }

    fn appearance_panel(&self, cx: &mut Context<Self>) -> Div {
        let colors = colors(cx);
        let appearances = [
            ("theme-light", Appearance::Light),
            ("theme-mint", Appearance::Mint),
            ("theme-dark", Appearance::Dark),
            ("theme-night", Appearance::Midnight),
        ];
        let outlines = [
            ("outline-cat", "猫咪头", Outline::Cat),
            ("outline-cut", "斜切", Outline::Cut),
            ("outline-rounded", "圆角", Outline::Rounded),
            ("outline-standard", "标准", Outline::Standard),
        ];
        panel(colors)
            .flex_shrink_0()
            .child(panel_header(colors, "外观与窗口", IconName::Palette))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .p(px(18.))
                    .gap(px(14.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(field_label("主题配色"))
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .gap(px(8.))
                                    .children(appearances.into_iter().map(|(id, appearance)| {
                                        let selected = self.preferences.appearance == appearance;
                                        let (bg_sw, panel_sw, accent_sw) = theme_swatch(appearance);
                                        let (fill, ink, border, hover) = if selected {
                                            (
                                                colors.selection,
                                                colors.ink,
                                                colors.selection_border,
                                                colors.selection_hover,
                                            )
                                        } else {
                                            (
                                                colors.panel,
                                                colors.muted,
                                                colors.border,
                                                colors.raised,
                                            )
                                        };
                                        button(id, cx)
                                            .custom(variant(cx, fill, ink, border, hover))
                                            .h(px(34.))
                                            .px(px(10.))
                                            .gap(px(7.))
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .size(px(18.))
                                                    .rounded_full()
                                                    .bg(rgb(bg_sw))
                                                    .border_1()
                                                    .border_color(rgb(colors.border))
                                                    .child(
                                                        div()
                                                            .size(px(8.))
                                                            .rounded_full()
                                                            .bg(rgb(accent_sw))
                                                            .border_1()
                                                            .border_color(rgb(panel_sw)),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(13.))
                                                    .text_color(rgb(ink))
                                                    .child(theme_name(appearance)),
                                            )
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.set_appearance(appearance, window, cx)
                                            }))
                                    })),
                            ),
                    )
                    .when(cfg!(windows), |this| {
                        this.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(8.))
                                .child(field_label("窗口轮廓"))
                                .child(
                                    div().flex().flex_wrap().gap(px(8.)).children(
                                        outlines.into_iter().map(|(id, label, outline)| {
                                            choice(
                                                id,
                                                label,
                                                self.preferences.outline == outline,
                                                cx,
                                            )
                                            .h(px(32.))
                                            .px(px(12.))
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.set_outline(outline, window, cx)
                                            }))
                                        }),
                                    ),
                                ),
                        )
                    })
                    .when_some(self.appearance_error.as_ref(), |this, error| {
                        this.child(error_line(colors, error.clone()))
                    }),
            )
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
        .gap(px(8.))
        .h(px(48.))
        .px(px(18.))
        .flex_shrink_0()
        .border_b_1()
        .border_color(rgb(colors.subtle_border))
        .child(icon(glyph, colors.success, 16.))
        .child(section_title(title))
}

pub(super) fn section_title(title: &'static str) -> Div {
    div()
        .text_size(px(15.5))
        .line_height(px(22.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(title)
}

fn field_label(text: &'static str) -> Div {
    div()
        .text_size(px(13.5))
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

fn toolbar_icon(
    id: &'static str,
    glyph: IconName,
    label: impl Into<SharedString>,
    cx: &App,
) -> Button {
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
        .h(px(34.))
        .p_0()
        .child(icon(glyph, colors.muted, 16.))
        .tooltip(label.into())
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
    let (fill, ink, border, hover) = if selected {
        (
            colors.selection,
            colors.ink,
            colors.selection_border,
            colors.selection_hover,
        )
    } else {
        (colors.panel, colors.muted, colors.border, colors.raised)
    };
    button(id, cx)
        .custom(variant(cx, fill, ink, border, hover))
        .child(
            div()
                .text_size(px(13.))
                .font_weight(if selected {
                    FontWeight::MEDIUM
                } else {
                    FontWeight::NORMAL
                })
                .text_color(rgb(ink))
                .child(label.into()),
        )
}

fn error_line(colors: Palette, text: String) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .text_size(px(12.5))
        .line_height(px(18.))
        .text_color(rgb(colors.error))
        .bg(rgb(colors.error_fill))
        .px(px(10.))
        .py(px(6.))
        .rounded(px(7.))
        .child(icon(IconName::TriangleAlert, colors.error, 14.))
        .child(text)
}

fn notice(colors: Palette, note: &Notice) -> AnyElement {
    if note.error {
        error_line(colors, note.text.clone()).into_any_element()
    } else {
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(10.))
            .py(px(6.))
            .rounded(px(7.))
            .bg(rgb(colors.success_fill))
            .text_size(px(12.5))
            .text_color(rgb(colors.success))
            .child(icon(IconName::CircleCheck, colors.success, 14.))
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
        .gap(px(3.))
        .when(divided && !narrow, |this| {
            this.pl(px(12.))
                .border_l_1()
                .border_color(rgb(colors.subtle_border))
        })
        .child(
            div()
                .text_size(px(11.5))
                .text_color(rgb(colors.muted))
                .child(label),
        )
        .child(
            div()
                .text_size(px(15.5))
                .line_height(px(21.))
                .font_weight(FontWeight::SEMIBOLD)
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

#[cfg(windows)]
fn restore_root_background() -> impl IntoElement {
    canvas(|_, _, _| {}, |_, _, _, cx| reset_root_background(cx))
        .absolute()
        .size_0()
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
                            for page in [
                                Page::Task,
                                Page::Tools,
                                Page::Results,
                                Page::Forensics,
                                Page::Settings,
                            ] {
                                bench.update(cx, |b, cx| {
                                    b.attack = 3;
                                    b.found_passwords = vec!["layout sample".into()];
                                    b.result_preview = Some(Ok((
                                        "$krb5asrep$23$PILINGSHAN@DAMDDOS.CTCT:7acf4d1b2b21a3a9a6352c06a20b52bd$2f4c93ed4f883f874f7714e78d36e81c7dfedc93fc10acfa640b6c227b731ec7309655e3c82591d89abcdef0123456789abcdef:SuperSecretPass2026\n".into(),
                                        false,
                                    )));
                                    b.page = page;
                                    cx.notify();
                                });
                                window.refresh();
                                window.draw(cx).clear();
                                assert_eq!(
                                    gpui_component::Theme::global(cx).background,
                                    gpui::transparent_black()
                                );
                            }
                            bench.update(cx, |b, cx| {
                                b.page = Page::Results;
                                b.result_wrap = false;
                                cx.notify();
                            });
                            window.refresh();
                            window.draw(cx).clear();
                            window.remove_window();
                        })
                        .unwrap();
                }
                cx.quit();
            });
    }
}
