use super::layout::{button, panel, primary, section_title};
use super::palette::colors;
use super::{FileSlot, Notice, Page, UpdateState, Workbench};
use crate::downloads::Resource;
use crate::updater;
use gpui::{
    AnyElement, AppContext, Context, Div, FontWeight, IntoElement, ParentElement, Styled, Window,
    div, prelude::FluentBuilder, px, rgb,
};
use gpui_component::{Disableable, Icon, IconName};
use std::path::PathBuf;

impl Workbench {
    pub(super) fn change_results_directory(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.engine_notice = Some(match self.locations.select(path, &self.data) {
            Ok(()) => {
                self.refresh_results();
                Notice {
                    text: "结果目录已更新。".into(),
                    error: false,
                }
            }
            Err(e) => Notice {
                text: format!("无法保存结果目录（{e}）。"),
                error: true,
            },
        });
        cx.notify();
    }

    pub(super) fn storage_panel(&self, cx: &mut Context<Self>) -> Div {
        let c = colors(cx);
        let active = self.locations.active(&self.data);
        let is_custom = active != self.data.join("jobs");
        panel(c)
            .flex_shrink_0()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(10.))
                    .h(px(48.))
                    .px(px(18.))
                    .flex_shrink_0()
                    .border_b_1()
                    .border_color(rgb(c.subtle_border))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                Icon::new(IconName::FolderOpen)
                                    .size(px(16.))
                                    .text_color(rgb(c.success)),
                            )
                            .child(section_title("恢复结果目录")),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                button("open-results-dir", cx)
                                    .h(px(30.))
                                    .px(px(9.))
                                    .gap(px(5.))
                                    .child(
                                        Icon::new(IconName::ExternalLink)
                                            .size(px(13.))
                                            .text_color(rgb(c.muted)),
                                    )
                                    .child("打开")
                                    .tooltip("在资源管理器中打开结果目录")
                                    .on_click(cx.listener(|this, _, _, cx| this.open_data_dir(cx))),
                            )
                            .when(is_custom, |row| {
                                row.child(
                                    button("default-results", cx)
                                        .h(px(30.))
                                        .px(px(9.))
                                        .label("恢复默认")
                                        .disabled(self.form_locked() || self.picking)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.change_results_directory(
                                                this.data.join("jobs"),
                                                cx,
                                            )
                                        })),
                                )
                            })
                            .child(
                                button("choose-results", cx)
                                    .h(px(30.))
                                    .px(px(10.))
                                    .label("更换目录")
                                    .disabled(self.form_locked() || self.picking)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.pick_file(FileSlot::ResultsDirectory, window, cx)
                                    })),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .p(px(18.))
                    .gap(px(10.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .px(px(12.))
                            .py(px(10.))
                            .rounded(px(8.))
                            .bg(rgb(c.raised))
                            .border_1()
                            .border_color(rgb(c.subtle_border))
                            .child(
                                Icon::new(IconName::Folder)
                                    .size(px(15.))
                                    .text_color(rgb(c.muted)),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .truncate()
                                    .font_family("Cascadia Mono")
                                    .text_size(px(12.))
                                    .text_color(rgb(c.ink))
                                    .child(active.display().to_string()),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(18.))
                            .text_color(rgb(c.muted))
                            .child("新任务的结果将写入此目录；更换后，历史记录仍可在「恢复结果」中查看。"),
                    ),
            )
    }

    pub(super) fn download_panel(&self, cx: &mut Context<Self>) -> Div {
        let c = colors(cx);
        panel(c)
            .flex_shrink_0()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(48.))
                    .px(px(18.))
                    .flex_shrink_0()
                    .border_b_1()
                    .border_color(rgb(c.subtle_border))
                    .child(
                        Icon::new(IconName::Inbox)
                            .size(px(16.))
                            .text_color(rgb(c.success)),
                    )
                    .child(section_title("内置资源下载")),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .p(px(18.))
                    .gap(px(10.))
                    .children(
                        [
                            (
                                Resource::Hashcat,
                                "hashcat",
                                "Hashcat 8.0.0",
                                if cfg!(target_os = "macos") {
                                    "macOS 请通过 brew install hashcat 安装"
                                } else {
                                    "官方发行包（含 Windows / Linux）"
                                },
                            ),
                            (
                                Resource::RockYou,
                                "rockyou",
                                "RockYou 字典",
                                "Kali Linux 提供的 rockyou.txt",
                            ),
                        ]
                        .into_iter()
                        .map(|(resource, id, label, source)| {
                            let installed = resource.installed(&self.data).is_file();
                            let active = self.download == Some(resource);
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap(px(12.))
                                .px(px(12.))
                                .py(px(10.))
                                .rounded(px(8.))
                                .bg(rgb(c.monitor))
                                .border_1()
                                .border_color(rgb(c.subtle_border))
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(3.))
                                        .min_w_0()
                                        .flex_1()
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap(px(8.))
                                                .child(
                                                    div()
                                                        .text_size(px(14.))
                                                        .font_weight(FontWeight::MEDIUM)
                                                        .child(label),
                                                )
                                                .when(installed, |row| {
                                                    row.child(
                                                        div()
                                                            .px(px(7.))
                                                            .py(px(1.))
                                                            .rounded_full()
                                                            .bg(rgb(c.success_fill))
                                                            .text_size(px(11.))
                                                            .font_weight(FontWeight::MEDIUM)
                                                            .text_color(rgb(c.success))
                                                            .child("已安装"),
                                                    )
                                                }),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(12.))
                                                .text_color(rgb(c.muted))
                                                .child(source),
                                        ),
                                )
                                .child(
                                    button(id, cx)
                                        .h(px(32.))
                                        .px(px(10.))
                                        .flex_shrink_0()
                                        .label(if active {
                                            "正在下载…"
                                        } else if installed {
                                            if resource == Resource::Hashcat {
                                                "使用此版本"
                                            } else {
                                                "用于字典任务"
                                            }
                                        } else {
                                            "下载安装"
                                        })
                                        .disabled(
                                            self.download.is_some()
                                                || self.form_locked()
                                                || self.probing
                                                || self.picking,
                                        )
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.get_resource(resource, window, cx)
                                        })),
                                )
                        }),
                    )
                    .when_some(self.download_note.as_ref(), |this, note| {
                        this.child(
                            div()
                                .px(px(10.))
                                .py(px(6.))
                                .rounded(px(6.))
                                .bg(rgb(if note.error {
                                    c.error_fill
                                } else {
                                    c.success_fill
                                }))
                                .text_size(px(12.))
                                .text_color(rgb(if note.error { c.error } else { c.success }))
                                .child(note.text.clone()),
                        )
                    }),
            )
    }

    pub(super) fn update_panel(&self, cx: &mut Context<Self>) -> Div {
        let c = colors(cx);
        let checking = matches!(self.update, UpdateState::Checking);
        let downloading = matches!(self.update, UpdateState::Downloading(_));
        let repo_url = format!("https://github.com/{}/releases", updater::REPO);

        let body: AnyElement = match &self.update {
            UpdateState::Idle => div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .p(px(12.))
                .rounded(px(8.))
                .bg(rgb(c.monitor))
                .border_1()
                .border_color(rgb(c.subtle_border))
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(rgb(c.muted))
                        .child(format!(
                            "当前版本 v{}，启动时自动从 GitHub ({}) 检查新版本。",
                            updater::CURRENT_VERSION,
                            updater::REPO
                        )),
                )
                .into_any_element(),
            UpdateState::Checking => div()
                .flex()
                .items_center()
                .gap(px(8.))
                .p(px(12.))
                .rounded(px(8.))
                .bg(rgb(c.monitor))
                .text_size(px(13.))
                .text_color(rgb(c.muted))
                .child(
                    Icon::new(IconName::LoaderCircle)
                        .size(px(15.))
                        .text_color(rgb(c.muted)),
                )
                .child("正在连接 GitHub 检查最新版本…")
                .into_any_element(),
            UpdateState::UpToDate => div()
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(12.))
                .py(px(10.))
                .rounded(px(8.))
                .bg(rgb(c.success_fill))
                .text_size(px(13.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(c.success))
                .child(
                    Icon::new(IconName::CircleCheck)
                        .size(px(15.))
                        .text_color(rgb(c.success)),
                )
                .child(format!(
                    "当前已是最新版本（v{}）",
                    updater::CURRENT_VERSION
                ))
                .into_any_element(),
            UpdateState::NoRelease => div()
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(12.))
                .py(px(10.))
                .rounded(px(8.))
                .bg(rgb(c.monitor))
                .border_1()
                .border_color(rgb(c.subtle_border))
                .text_size(px(13.))
                .text_color(rgb(c.muted))
                .child(
                    Icon::new(IconName::Info)
                        .size(px(15.))
                        .text_color(rgb(c.muted)),
                )
                .child(format!(
                    "GitHub 仓库尚未发布正式 Release（当前版本 v{}）。",
                    updater::CURRENT_VERSION
                ))
                .into_any_element(),
            UpdateState::Available(release) => {
                let html_url = release.html_url.clone();
                let asset_info = release.asset.as_ref().map(|a| {
                    if a.size > 0 {
                        format!("{} · {}", a.name, updater::format_size(a.size))
                    } else {
                        a.name.clone()
                    }
                });
                let has_asset = release.asset.is_some();
                let notes_preview = release
                    .notes
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .take(4)
                    .collect::<Vec<_>>()
                    .join("\n");
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .p(px(14.))
                    .rounded(px(9.))
                    .bg(rgb(c.selection))
                    .border_1()
                    .border_color(rgb(c.selection_border))
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
                                    .child(
                                        div()
                                            .px(px(8.))
                                            .py(px(2.))
                                            .rounded_full()
                                            .bg(rgb(c.accent))
                                            .font_family("Cascadia Mono")
                                            .text_size(px(12.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(rgb(c.accent_ink))
                                            .child(format!("发现新版本 v{}", release.version)),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(13.5))
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(release.title.clone()),
                                    ),
                            )
                            .when_some(asset_info, |row, info| {
                                row.child(
                                    div()
                                        .font_family("Cascadia Mono")
                                        .text_size(px(11.5))
                                        .text_color(rgb(c.muted))
                                        .child(info),
                                )
                            }),
                    )
                    .when(!notes_preview.is_empty(), |card| {
                        card.child(
                            div()
                                .p(px(10.))
                                .rounded(px(7.))
                                .bg(rgb(c.panel))
                                .border_1()
                                .border_color(rgb(c.subtle_border))
                                .text_size(px(12.5))
                                .line_height(px(19.))
                                .text_color(rgb(c.muted))
                                .child(notes_preview),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(8.))
                            .when(has_asset, |row| {
                                row.child(
                                    primary("download-app-update", cx)
                                        .h(px(34.))
                                        .px(px(14.))
                                        .gap(px(6.))
                                        .label("下载并更新")
                                        .disabled(self.cancel.is_some())
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.download_update(window, cx)
                                        })),
                                )
                            })
                            .child(
                                button("open-release-page", cx)
                                    .h(px(34.))
                                    .px(px(12.))
                                    .gap(px(5.))
                                    .child(
                                        Icon::new(IconName::ExternalLink)
                                            .size(px(13.))
                                            .text_color(rgb(c.muted)),
                                    )
                                    .child("在 GitHub 查看")
                                    .on_click(move |_, _, cx| cx.open_url(&html_url)),
                            ),
                    )
                    .into_any_element()
            }
            UpdateState::Downloading(release) => div()
                .flex()
                .items_center()
                .gap(px(8.))
                .p(px(12.))
                .rounded(px(8.))
                .bg(rgb(c.selection))
                .border_1()
                .border_color(rgb(c.selection_border))
                .text_size(px(13.))
                .text_color(rgb(c.ink))
                .child(
                    Icon::new(IconName::LoaderCircle)
                        .size(px(15.))
                        .text_color(rgb(c.ink)),
                )
                .child(format!(
                    "正在从 GitHub 下载并校验 Lovelycat v{}…",
                    release.version
                ))
                .into_any_element(),
            UpdateState::Ready(prepared) => {
                let can_restart = !cfg!(target_os = "macos") && prepared.binary_path.is_some();
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .flex_wrap()
                    .gap(px(10.))
                    .p(px(12.))
                    .rounded(px(8.))
                    .bg(rgb(c.success_fill))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(c.success))
                            .child(
                                Icon::new(IconName::CircleCheck)
                                    .size(px(16.))
                                    .text_color(rgb(c.success)),
                            )
                            .child(format!(
                                "新版本 v{} 已下载并校验完成",
                                prepared.version
                            )),
                    )
                    .child(
                        primary("apply-app-update", cx)
                            .h(px(34.))
                            .px(px(14.))
                            .label(if can_restart {
                                "立即重启更新"
                            } else {
                                "打开安装包目录"
                            })
                            .disabled(self.cancel.is_some())
                            .on_click(cx.listener(|this, _, _, cx| this.apply_update(cx))),
                    )
                    .into_any_element()
            }
            UpdateState::Failed(error) => div()
                .flex()
                .items_center()
                .gap(px(7.))
                .px(px(12.))
                .py(px(9.))
                .rounded(px(8.))
                .bg(rgb(c.error_fill))
                .text_size(px(12.5))
                .text_color(rgb(c.error))
                .child(
                    Icon::new(IconName::TriangleAlert)
                        .size(px(14.))
                        .text_color(rgb(c.error)),
                )
                .child(error.clone())
                .into_any_element(),
        };

        panel(c)
            .flex_shrink_0()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(10.))
                    .h(px(48.))
                    .px(px(18.))
                    .flex_shrink_0()
                    .border_b_1()
                    .border_color(rgb(c.subtle_border))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                Icon::new(IconName::GitHub)
                                    .size(px(16.))
                                    .text_color(rgb(c.success)),
                            )
                            .child(section_title("版本更新"))
                            .child(
                                div()
                                    .px(px(8.))
                                    .py(px(2.))
                                    .rounded_full()
                                    .bg(rgb(c.raised))
                                    .font_family("Cascadia Mono")
                                    .text_size(px(11.5))
                                    .text_color(rgb(c.muted))
                                    .child(format!("v{}", updater::CURRENT_VERSION)),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                button("open-github-releases", cx)
                                    .h(px(30.))
                                    .px(px(9.))
                                    .gap(px(5.))
                                    .child(
                                        Icon::new(IconName::ExternalLink)
                                            .size(px(13.))
                                            .text_color(rgb(c.muted)),
                                    )
                                    .child("发布页")
                                    .tooltip("在浏览器中打开 GitHub Releases 页面")
                                    .on_click(move |_, _, cx| cx.open_url(&repo_url)),
                            )
                            .child(
                                button("check-app-update", cx)
                                    .h(px(30.))
                                    .px(px(10.))
                                    .gap(px(5.))
                                    .child(
                                        Icon::new(IconName::Redo2)
                                            .size(px(13.))
                                            .text_color(rgb(c.success)),
                                    )
                                    .child(if checking {
                                        "检查中…"
                                    } else {
                                        "检查更新"
                                    })
                                    .disabled(checking || downloading)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.check_for_updates(true, window, cx)
                                    })),
                            ),
                    ),
            )
            .child(div().p(px(18.)).child(body))
    }

    fn get_resource(&mut self, resource: Resource, window: &mut Window, cx: &mut Context<Self>) {
        if self.download.is_some() || self.form_locked() || self.probing {
            return;
        }
        let local = resource.installed(&self.data);
        if local.is_file() {
            if resource == Resource::Hashcat {
                self.connect(local, true, window, cx);
            } else {
                self.words = Some(local);
                self.activate_attack(0, window, cx);
                self.change_page(Page::Task, cx);
            }
            return;
        }
        self.download = Some(resource);
        self.download_note = None;
        let data = self.data.clone();
        let job = cx.background_spawn(async move { resource.install(&data) });
        cx.spawn_in(window, async move |weak, cx| {
            let result = job.await;
            let _ = weak.update_in(cx, |this, window, cx| {
                this.download = None;
                this.download_note = Some(match result {
                    Ok(path) => {
                        // A task may have been started while the download was in flight.
                        let text = if resource == Resource::Hashcat
                            && !this.form_locked()
                            && !this.probing
                            && !this.picking
                        {
                            this.connect(path, true, window, cx);
                            "Hashcat 已安装，正在连接。".to_owned()
                        } else {
                            format!("已安装至 {}", path.display())
                        };
                        Notice { text, error: false }
                    }
                    Err(text) => Notice { text, error: true },
                });
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}
