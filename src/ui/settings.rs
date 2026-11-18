use super::layout::{button, panel, section_title};
use super::palette::colors;
use super::{FileSlot, Notice, Page, Workbench};
use crate::downloads::Resource;
use gpui::{
    AppContext, Context, Div, ParentElement, Styled, Window, div, prelude::FluentBuilder, px, rgb,
};
use gpui_component::Disableable;
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
        panel(c)
            .p(px(16.))
            .gap(px(8.))
            .flex_shrink_0()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(section_title("恢复结果目录"))
                    .child(
                        div()
                            .flex()
                            .gap(px(6.))
                            .when(active != self.data.join("jobs"), |row| {
                                row.child(
                                    button("default-results", cx)
                                        .h(px(32.))
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
                                    .h(px(32.))
                                    .label("更换目录")
                                    .disabled(self.form_locked() || self.picking)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.pick_file(FileSlot::ResultsDirectory, window, cx)
                                    })),
                            ),
                    ),
            )
            .child(div().text_size(px(14.)).child(active.display().to_string()))
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(rgb(c.muted))
                    .child("新任务写入此目录。旧记录仍可在恢复结果中查看。"),
            )
    }

    pub(super) fn download_panel(&self, cx: &mut Context<Self>) -> Div {
        let c = colors(cx);
        panel(c)
            .p(px(16.))
            .gap(px(10.))
            .flex_shrink_0()
            .child(section_title("下载资源"))
            .children(
                [
                    (
                        Resource::Hashcat,
                        "hashcat",
                        "Hashcat 8.0.0",
                        "官方 Windows 发行包",
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
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .child(div().text_size(px(14.)).child(label))
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
                                .label(if active {
                                    "正在下载与校验…"
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
                        .text_size(px(13.))
                        .text_color(rgb(if note.error { c.error } else { c.success }))
                        .child(note.text.clone()),
                )
            })
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
