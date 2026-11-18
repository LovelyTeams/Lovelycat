use std::path::{Path, PathBuf};

use gpui::{
    AppContext, ClipboardItem, Context, Div, Entity, ExternalPaths, FontWeight, InteractiveElement,
    IntoElement, ParentElement, PathPromptOptions, StatefulInteractiveElement, Styled, Window, div,
    prelude::FluentBuilder, px, relative, rgb,
};
use gpui_component::{
    Disableable,
    input::{InputEvent, InputState},
};

use super::layout::{button, choice, panel, primary, section_title, single_line};
use super::palette::{Palette, colors};
use super::{Notice, Workbench, hashcat};
use crate::forensics::{self, Credential, Encryption, ExportRequest, Inventory, Method, Volume};
use crate::process::Cancel;

enum Activity {
    Idle,
    Installing,
    Inspecting(Cancel),
    Exporting(Cancel),
}

impl Activity {
    fn busy(&self) -> bool {
        !matches!(self, Self::Idle)
    }
}

#[derive(Clone, Copy)]
enum Pick {
    Image,
    KeyFile,
}

struct Receipt {
    path: PathBuf,
    sha256: String,
    size: u64,
    record: String,
}

enum ExportEvent {
    Progress(forensics::Progress),
    Done(Result<forensics::Exported, String>),
}

pub(super) struct Forensics {
    installed: bool,
    activity: Activity,
    stopping: bool,
    image: Option<PathBuf>,
    inventory: Option<Inventory>,
    selected: Option<usize>,
    method: Method,
    secret: Entity<InputState>,
    key_file: Option<PathBuf>,
    progress: Option<forensics::Progress>,
    receipt: Option<Receipt>,
    notice: Option<Notice>,
}

impl Forensics {
    pub(super) fn new(data: &Path, window: &mut Window, cx: &mut Context<Workbench>) -> Self {
        Self {
            installed: forensics::runtime_path(data).is_file(),
            activity: Activity::Idle,
            stopping: false,
            image: None,
            inventory: None,
            selected: None,
            method: Method::Password,
            secret: masked_input(Method::Password, window, cx),
            key_file: None,
            progress: None,
            receipt: None,
            notice: None,
        }
    }

    fn volume(&self) -> Option<&Volume> {
        self.inventory.as_ref()?.volumes.get(self.selected?)
    }

    fn clear_credential(&mut self, window: &mut Window, cx: &mut Context<Workbench>) {
        self.secret = masked_input(self.method, window, cx);
        self.key_file = None;
    }

    fn failed(&mut self, text: String) {
        self.notice = Some(Notice {
            error: text != forensics::CANCELLED,
            text,
        });
    }
}

fn masked_input(
    method: Method,
    window: &mut Window,
    cx: &mut Context<Workbench>,
) -> Entity<InputState> {
    let input = cx.new(|cx| {
        InputState::new(window, cx)
            .masked(true)
            .placeholder(match method {
                Method::RecoveryPassword => "输入 48 位数字恢复密钥",
                _ => "输入卷的已知密码",
            })
    });
    cx.subscribe(&input, |_, _, event: &InputEvent, cx| {
        if matches!(event, InputEvent::Change) {
            cx.notify();
        }
    })
    .detach();
    input
}

fn method_label(method: Method, kind: Encryption) -> &'static str {
    match method {
        Method::Password => "密码",
        Method::RecoveryPassword => "48 位恢复密钥",
        Method::KeyFile if kind == Encryption::Bitlocker => "BEK 文件",
        Method::KeyFile => "密钥文件",
    }
}

fn size_label(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.2} GiB", bytes as f64 / (1024_f64.powi(3)))
    } else if bytes >= 1024 * 1024 {
        format!("{:.2} MiB", bytes as f64 / (1024_f64.powi(2)))
    } else if bytes >= 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.)
    } else {
        format!("{bytes} B")
    }
}

fn volume_label(volume: &Volume) -> String {
    let label = match volume.number {
        Some(number) => format!("分区 {number}"),
        None => "整个卷".to_owned(),
    };
    if volume.name.is_empty() {
        label
    } else {
        format!("{label} · {}", volume.name)
    }
}

fn detail(colors: Palette, text: impl Into<gpui::SharedString>) -> Div {
    div()
        .text_size(px(13.))
        .line_height(px(19.))
        .text_color(rgb(colors.muted))
        .child(text.into())
}

impl Workbench {
    pub(super) fn forensics_page(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = colors(cx);
        let state = &self.forensics;
        let locked = self.picking || state.activity.busy();
        let stacked = window.viewport_size().width < px(940.);
        let image = panel(c)
            .p(px(16.))
            .gap(px(12.))
            .flex_shrink_0()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(section_title("证据镜像"))
                    .child(
                        button("forensic-image-pick", cx)
                            .h(px(36.))
                            .label("选择镜像")
                            .disabled(locked || !state.installed)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.pick_forensic_file(Pick::Image, window, cx)
                            })),
                    ),
            )
            .child(
                div()
                    .id("forensic-image-drop")
                    .bg(rgb(c.raised))
                    .rounded(px(8.))
                    .p(px(12.))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .when(state.installed && !locked, |area| {
                        area.on_drop(cx.listener(|this, files: &ExternalPaths, window, cx| {
                            if let [path] = files.paths() {
                                this.inspect_forensic_image(path.clone(), window, cx);
                            } else {
                                this.forensics.failed("请选择一个镜像文件。".into());
                                cx.notify();
                            }
                        }))
                    })
                    .child(
                        div().truncate().font_weight(FontWeight::MEDIUM).child(
                            state
                                .image
                                .as_deref()
                                .map(hashcat::file_name)
                                .unwrap_or_else(|| {
                                    if state.installed {
                                        "拖入磁盘镜像"
                                    } else {
                                        "安装组件后即可导入镜像"
                                    }
                                    .into()
                                }),
                        ),
                    )
                    .child(
                        detail(
                            c,
                            state
                                .image
                                .as_ref()
                                .map(|path| path.display().to_string())
                                .unwrap_or_else(|| "RAW / DD / IMG、E01、VHD 和 VHDX".into()),
                        )
                        .truncate(),
                    ),
            )
            .when_some(state.inventory.as_ref(), |card, inventory| {
                let label = format!(
                    "{}  ·  {}  ·  {} 字节扇区  ·  {}",
                    inventory.container,
                    inventory.table.as_deref().unwrap_or("卷镜像"),
                    inventory.sector_size,
                    size_label(inventory.size)
                );
                let label = if inventory.segments > 1 {
                    format!("{label}  ·  {} 个分段", inventory.segments)
                } else {
                    label
                };
                card.child(detail(c, label))
            })
            .when(matches!(state.activity, Activity::Inspecting(_)), |card| {
                card.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(detail(
                            c,
                            if state.stopping {
                                "正在取消…"
                            } else {
                                "正在识别镜像…"
                            },
                        ))
                        .child(self.cancel_disk_button(cx)),
                )
            });

        div()
            .id("forensic-scroll")
            .min_h_0()
            .flex_1()
            .overflow_y_scroll()
            .px(px(24.))
            .pb(px(24.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .when(!state.installed, |page| {
                page.child(
                    panel(c)
                        .p(px(16.))
                        .flex_shrink_0()
                        .gap(px(12.))
                        .items_start()
                        .child(section_title("磁盘取证组件"))
                        .child(detail(
                            c,
                            "首次使用需下载约 14 MB。组件安装在 Lovelycat 的应用目录。",
                        ))
                        .child(
                            primary("forensic-setup", cx)
                                .h(px(36.))
                                .label(if state.activity.busy() {
                                    "正在下载与校验…"
                                } else {
                                    "安装组件"
                                })
                                .disabled(locked)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.install_forensics(window, cx)
                                })),
                        ),
                )
            })
            .child(image)
            .when_some(state.notice.as_ref(), |page, note| {
                page.child(
                    div()
                        .text_size(px(13.))
                        .line_height(px(20.))
                        .text_color(rgb(if note.error { c.error } else { c.success }))
                        .child(note.text.clone()),
                )
            })
            .when_some(state.inventory.as_ref(), |page, inventory| {
                page.child(
                    div()
                        .flex()
                        .flex_shrink_0()
                        .items_start()
                        .gap(px(12.))
                        .when(stacked, |row| row.flex_col())
                        .child(
                            self.forensic_volumes(c, inventory, cx)
                                .when(stacked || state.volume().is_none(), |list| list.w_full())
                                .when(!stacked && state.volume().is_some(), |list| {
                                    list.flex_1().min_w_0()
                                }),
                        )
                        .when_some(state.volume(), |row, volume| {
                            row.child(
                                self.forensic_export_panel(c, volume, cx)
                                    .when(stacked, |card| card.w_full())
                                    .when(!stacked, |card| card.flex_1().min_w_0()),
                            )
                        }),
                )
            })
    }

    fn forensic_volumes(&self, c: Palette, inventory: &Inventory, cx: &mut Context<Self>) -> Div {
        let state = &self.forensics;
        panel(c)
            .p(px(16.))
            .gap(px(10.))
            .child(section_title("分区与加密卷"))
            .children(inventory.volumes.iter().enumerate().map(|(index, volume)| {
                let selected = state.selected == Some(index);
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .rounded(px(7.))
                    .p(px(10.))
                    .bg(rgb(if selected { c.selection } else { c.raised }))
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
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(volume_label(volume)),
                            )
                            .child(detail(
                                c,
                                format!(
                                    "{}  ·  {}  ·  偏移 0x{:X}",
                                    volume.kind.label(),
                                    size_label(volume.size),
                                    volume.offset,
                                ),
                            ))
                            .when_some(volume.issue.as_ref(), |row, issue| {
                                row.child(detail(c, issue.clone()))
                            }),
                    )
                    .when(!volume.methods.is_empty(), |row| {
                        row.child(
                            choice(
                                ("forensic-volume", index),
                                if selected { "已选" } else { "选择" },
                                selected,
                                cx,
                            )
                            .h(px(30.))
                            .disabled(self.picking || state.activity.busy())
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.select_forensic_volume(index, window, cx)
                                },
                            )),
                        )
                    })
            }))
            .when(
                !inventory
                    .volumes
                    .iter()
                    .any(|volume| !volume.methods.is_empty()),
                |card| {
                    card.child(detail(
                        c,
                        "此镜像中没有可使用已知凭据解密的 BitLocker 或 LUKS 卷。",
                    ))
                },
            )
    }

    fn forensic_export_panel(&self, c: Palette, volume: &Volume, cx: &mut Context<Self>) -> Div {
        let state = &self.forensics;
        let locked = self.picking || state.activity.busy();
        let has_credential = match state.method {
            Method::KeyFile => state.key_file.is_some(),
            _ => !state.secret.read(cx).value().is_empty(),
        };
        panel(c)
            .p(px(16.))
            .gap(px(12.))
            .child(section_title("解密导出"))
            .child(detail(
                c,
                format!("{} · {}", volume_label(volume), volume.kind.label()),
            ))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(6.))
                    .children(volume.methods.iter().map(|&method| {
                        let label = method_label(method, volume.kind);
                        choice(
                            ("forensic-credential", method as usize),
                            label,
                            state.method == method,
                            cx,
                        )
                        .h(px(32.))
                        .disabled(locked)
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                if this.forensics.method != method {
                                    this.forensics.method = method;
                                    this.forensics.clear_credential(window, cx);
                                    this.forensics.notice = None;
                                    cx.notify();
                                }
                            },
                        ))
                    })),
            )
            .when(state.method == Method::KeyFile, |card| {
                card.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            button("forensic-key-file", cx)
                                .h(px(36.))
                                .label("选择密钥文件")
                                .disabled(locked)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.pick_forensic_file(Pick::KeyFile, window, cx)
                                })),
                        )
                        .when_some(state.key_file.as_deref(), |row, path| {
                            row.child(detail(c, hashcat::file_name(path)).min_w_0().truncate())
                        }),
                )
            })
            .when(state.method != Method::KeyFile, |card| {
                card.child(single_line(&state.secret).disabled(locked))
            })
            .child(detail(
                c,
                "以只读方式打开源镜像，将选中的卷保存为新 RAW 文件。",
            ))
            .when(matches!(state.activity, Activity::Exporting(_)), |card| {
                let status = if state.stopping {
                    "正在取消…".to_owned()
                } else if let Some(progress) = state.progress {
                    format!(
                        "正在导出  {} / {}",
                        size_label(progress.done),
                        size_label(progress.total)
                    )
                } else {
                    "正在验证凭据…".to_owned()
                };
                card.child(detail(c, status))
                    .when_some(state.progress, |card, progress| {
                        let fraction =
                            (progress.done as f32 / progress.total.max(1) as f32).clamp(0., 1.);
                        card.child(
                            div()
                                .w_full()
                                .h(px(5.))
                                .rounded_full()
                                .bg(rgb(c.raised))
                                .child(
                                    div()
                                        .w(relative(fraction))
                                        .h_full()
                                        .rounded_full()
                                        .bg(rgb(c.accent)),
                                ),
                        )
                    })
                    .child(div().flex().child(self.cancel_disk_button(cx)))
            })
            .when(!matches!(state.activity, Activity::Exporting(_)), |card| {
                card.child(
                    div().flex().child(
                        primary("forensic-export", cx)
                            .h(px(40.))
                            .label("解密并导出")
                            .disabled(locked || !has_credential)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.choose_forensic_destination(window, cx)
                            })),
                    ),
                )
            })
            .when_some(state.receipt.as_ref(), |card, receipt| {
                let path = receipt.path.clone();
                let record = receipt.record.clone();
                card.child(
                    div()
                        .border_t_1()
                        .border_color(rgb(c.border))
                        .pt(px(12.))
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(
                            div()
                                .text_color(rgb(c.success))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(format!("已导出  {}", size_label(receipt.size))),
                        )
                        .child(detail(c, receipt.path.display().to_string()).truncate())
                        .child(detail(c, "SHA-256"))
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_family("Consolas")
                                .child(receipt.sha256.clone()),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap(px(6.))
                                .child(
                                    button("forensic-reveal", cx)
                                        .h(px(32.))
                                        .label("查看文件")
                                        .on_click(move |_, _, cx| cx.reveal_path(&path)),
                                )
                                .child(
                                    button("forensic-copy", cx)
                                        .h(px(32.))
                                        .label("复制取证记录")
                                        .on_click(move |_, _, cx| {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                record.clone(),
                                            ));
                                        }),
                                ),
                        ),
                )
            })
    }

    fn cancel_disk_button(&self, cx: &mut Context<Self>) -> gpui_component::button::Button {
        button("forensic-cancel", cx)
            .h(px(32.))
            .label("取消")
            .disabled(self.forensics.stopping)
            .on_click(cx.listener(|this, _, _, cx| {
                if let Activity::Inspecting(cancel) | Activity::Exporting(cancel) =
                    &this.forensics.activity
                {
                    cancel.stop();
                    this.forensics.stopping = true;
                    cx.notify();
                }
            }))
    }

    fn install_forensics(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.forensics.activity.busy() {
            return;
        }
        self.forensics.activity = Activity::Installing;
        self.forensics.notice = None;
        let data = self.data.clone();
        let task = cx.background_spawn(async move { forensics::install(&data) });
        cx.spawn_in(window, async move |weak, cx| {
            let result = task.await;
            let _ = weak.update_in(cx, move |this, _, cx| {
                this.forensics.activity = Activity::Idle;
                match result {
                    Ok(()) => this.forensics.installed = true,
                    Err(error) => this.forensics.failed(error),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn pick_forensic_file(&mut self, pick: Pick, window: &mut Window, cx: &mut Context<Self>) {
        if self.picking || !self.forensics.installed || self.forensics.activity.busy() {
            return;
        }
        self.picking = true;
        let prompt = match pick {
            Pick::Image => "选择磁盘镜像",
            Pick::KeyFile => "选择卷的密钥文件",
        };
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(prompt.into()),
        });
        cx.spawn_in(window, async move |weak, cx| {
            let result = picker.await;
            let _ = weak.update_in(cx, move |this, window, cx| {
                this.picking = false;
                match result {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.into_iter().next() {
                            match pick {
                                Pick::Image => this.inspect_forensic_image(path, window, cx),
                                Pick::KeyFile => {
                                    this.forensics.key_file = Some(path);
                                    this.forensics.notice = None;
                                }
                            }
                        }
                    }
                    Ok(Ok(None)) => {}
                    _ => this.forensics.failed("无法打开文件选择器。".into()),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn inspect_forensic_image(
        &mut self,
        image: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let state = &mut self.forensics;
        if self.picking || !state.installed || state.activity.busy() {
            return;
        }
        let (cancel, flag) = Cancel::pair();
        state.activity = Activity::Inspecting(cancel);
        state.inventory = None;
        state.selected = None;
        state.notice = None;
        state.receipt = None;
        state.image = Some(image.clone());
        state.clear_credential(window, cx);
        let data = self.data.clone();
        let task = cx.background_spawn(async move { forensics::inspect(&data, &image, &flag) });
        cx.spawn_in(window, async move |weak, cx| {
            let result = task.await;
            let _ = weak.update_in(cx, move |this, window, cx| {
                this.forensics.activity = Activity::Idle;
                this.forensics.stopping = false;
                match result {
                    Ok(inventory) => {
                        let first = inventory
                            .volumes
                            .iter()
                            .position(|volume| !volume.methods.is_empty());
                        this.forensics.inventory = Some(inventory);
                        if let Some(index) = first {
                            this.select_forensic_volume(index, window, cx);
                        }
                    }
                    Err(error) => this.forensics.failed(error),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn select_forensic_volume(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let state = &mut self.forensics;
        if self.picking || state.activity.busy() || state.selected == Some(index) {
            return;
        }
        let Some(&method) = state
            .inventory
            .as_ref()
            .and_then(|inventory| inventory.volumes.get(index))
            .and_then(|volume| volume.methods.first())
        else {
            return;
        };
        state.selected = Some(index);
        state.method = method;
        state.receipt = None;
        state.notice = None;
        state.clear_credential(window, cx);
        cx.notify();
    }

    fn choose_forensic_destination(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = &self.forensics;
        if self.picking || state.activity.busy() {
            return;
        }
        let (Some(image), Some(volume)) = (state.image.as_ref(), state.volume()) else {
            return;
        };
        let name = match volume.number {
            Some(number) => format!("volume-{number}-decrypted.img"),
            None => "volume-decrypted.img".into(),
        };
        let configured = self.locations.active(&self.data);
        let directory = if configured.is_dir() {
            configured.as_path()
        } else {
            image.parent().unwrap_or(&self.data)
        };
        let picker = cx.prompt_for_new_path(directory, Some(&name));
        self.picking = true;
        cx.spawn_in(window, async move |weak, cx| {
            let result = picker.await;
            let _ = weak.update_in(cx, move |this, window, cx| {
                this.picking = false;
                match result {
                    Ok(Ok(Some(path))) => this.export_forensic_volume(path, window, cx),
                    Ok(Ok(None)) => {}
                    _ => this.forensics.failed("无法打开保存对话框。".into()),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn export_forensic_volume(
        &mut self,
        destination: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let state = &mut self.forensics;
        if state.activity.busy() {
            return;
        }
        let (Some(image), Some(inventory), Some(volume)) = (
            state.image.clone(),
            state.inventory.as_ref(),
            state.volume().cloned(),
        ) else {
            return;
        };
        let credential = match state.method {
            Method::Password | Method::RecoveryPassword => {
                let value = state.secret.read(cx).value();
                if value.is_empty() || value.len() > 8 * 1024 {
                    state.failed("请输入有效的卷密码或恢复密钥。".into());
                    cx.notify();
                    return;
                }
                if state.method == Method::RecoveryPassword {
                    Credential::RecoveryPassword(value.trim().to_owned())
                } else {
                    Credential::Password(value.to_string())
                }
            }
            Method::KeyFile => {
                let Some(path) = state.key_file.clone() else {
                    return;
                };
                Credential::KeyFile(path)
            }
        };
        let context = format!(
            "源镜像 {}\n镜像容器 {}（{} 段）\n卷 {} · {}\n源卷偏移 {} 字节\n源卷长度 {} 字节",
            image.display(),
            inventory.container,
            inventory.segments,
            volume_label(&volume),
            volume.kind.label(),
            volume.offset,
            volume.size,
        );
        let request = ExportRequest {
            path: image,
            stamp: inventory.stamp.clone(),
            volume,
            credential,
            destination: destination.clone(),
        };
        let (cancel, flag) = Cancel::pair();
        state.activity = Activity::Exporting(cancel);
        state.progress = None;
        state.receipt = None;
        state.notice = None;
        state.clear_credential(window, cx);
        let data = self.data.clone();
        let (tx, rx) = async_channel::unbounded();
        cx.background_spawn(async move {
            let result = forensics::export(&data, request, &flag, |progress| {
                let _ = tx.try_send(ExportEvent::Progress(progress));
            });
            let _ = tx.try_send(ExportEvent::Done(result));
        })
        .detach();
        cx.spawn_in(window, async move |weak, cx| {
            while let Ok(event) = rx.recv().await {
                let Ok(finished) = weak.update_in(cx, |this, _, cx| {
                    let finished = match event {
                        ExportEvent::Progress(progress) => {
                            this.forensics.progress = Some(progress);
                            false
                        }
                        ExportEvent::Done(result) => {
                            this.forensics.activity = Activity::Idle;
                            this.forensics.stopping = false;
                            match result {
                                Ok(export) => {
                                    let record = format!(
                                        "Lovelycat 磁盘取证\n{context}\n导出文件 {}\n导出长度 {} 字节\nSHA-256 {}\n完成时间 {}",
                                        destination.display(), export.bytes_written, export.sha256,
                                        chrono::Utc::now().to_rfc3339(),
                                    );
                                    this.forensics.receipt = Some(Receipt {
                                        path: destination.clone(),
                                        sha256: export.sha256,
                                        size: export.bytes_written,
                                        record,
                                    });
                                }
                                Err(error) => this.forensics.failed(error),
                            }
                            true
                        }
                    };
                    cx.notify();
                    finished
                }) else {
                    break;
                };
                if finished {
                    break;
                }
            }
        }).detach();
        cx.notify();
    }
}
