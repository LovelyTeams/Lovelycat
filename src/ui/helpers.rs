use std::{
    collections::HashSet,
    fs,
    io::{BufWriter, Write},
    path::Path,
};

use super::{
    Workbench,
    layout::{button, choice, multiline, single_line},
    palette::colors,
};
use gpui::{
    AppContext, Context, Div, Entity, FontWeight, ParentElement, Styled, Window, div,
    prelude::FluentBuilder, px, rgb,
};
use gpui_component::{
    Disableable, Icon, IconName,
    input::{InputEvent, InputState},
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Artifact {
    Words,
    Rules,
}

pub(super) struct QuickSetup {
    expanded: bool,
    seeds: Entity<InputState>,
    suffixes: Entity<InputState>,
    capital: bool,
    upper: bool,
    note: String,
}

impl QuickSetup {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Workbench>) -> Self {
        let seeds = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .rows(2)
                .placeholder("填写你记得的基础词，每行一个")
        });
        let suffixes = cx.new(|cx| InputState::new(window, cx).placeholder("可选后缀，用逗号分隔"));
        for input in [&seeds, &suffixes] {
            cx.subscribe(input, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.helpers.note.clear();
                    cx.notify();
                }
            })
            .detach();
        }
        Self {
            expanded: false,
            seeds,
            suffixes,
            capital: false,
            upper: false,
            note: String::new(),
        }
    }
}

#[derive(Clone)]
struct Generator {
    seeds: String,
    suffixes: String,
    capital: bool,
    upper: bool,
}

impl Generator {
    fn visit(
        &self,
        kind: Artifact,
        mut emit: impl FnMut(&str) -> Result<bool, String>,
    ) -> Result<(), String> {
        let mut endings = vec![""];
        for suffix in self
            .suffixes
            .split([',', '，', '\n'])
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            if !endings.contains(&suffix) {
                endings.push(suffix);
            }
        }
        if kind == Artifact::Rules {
            for suffix in &endings {
                if suffix.len() > 30 || !suffix.bytes().all(|b| b.is_ascii_graphic()) {
                    return Err(
                        "规则后缀支持最多 30 个可见 ASCII 字符。含中文的后缀可直接保存为字典。"
                            .into(),
                    );
                }
            }
            for (cmd, enabled) in [(":", true), ("c", self.capital), ("u", self.upper)] {
                if !enabled {
                    continue;
                }
                for suffix in &endings {
                    let mut rule = String::new();
                    if cmd != ":" || suffix.is_empty() {
                        rule.push_str(cmd);
                    }
                    for ch in suffix.chars() {
                        rule.push('$');
                        rule.push(ch);
                    }
                    if !emit(&rule)? {
                        return Ok(());
                    }
                }
            }
        } else {
            let mut seen = HashSet::new();
            let mut any = false;
            for word in self.seeds.lines().filter(|s| !s.is_empty()) {
                any = true;
                let mut forms = vec![word.to_owned()];
                if self.capital {
                    let mut chars = word.chars();
                    if let Some(first) = chars.next() {
                        forms.push(format!(
                            "{}{}",
                            first.to_uppercase(),
                            chars.as_str().to_lowercase()
                        ));
                    }
                }
                if self.upper {
                    forms.push(word.to_uppercase());
                }
                for form in forms {
                    for suffix in &endings {
                        let word = format!("{form}{suffix}");
                        if seen.insert(word.clone()) && !emit(&word)? {
                            return Ok(());
                        }
                    }
                }
            }
            if !any {
                return Err("请先填写至少一个基础词。".into());
            }
        }
        Ok(())
    }

    fn save(&self, kind: Artifact, path: &Path) -> Result<(), String> {
        let mut tmp = tempfile::NamedTempFile::new_in(path.parent().ok_or("缺少保存目录")?)
            .map_err(|e| e.to_string())?;
        {
            let mut writer = BufWriter::new(tmp.as_file_mut());
            self.visit(kind, |line| {
                writeln!(writer, "{line}").map_err(|e| e.to_string())?;
                Ok(true)
            })?;
            writer.flush().map_err(|e| e.to_string())?;
        }
        tmp.persist(path).map_err(|e| e.to_string())?;
        Ok(())
    }
}

impl Workbench {
    fn generator(&self, cx: &Context<Self>) -> Generator {
        Generator {
            seeds: self.helpers.seeds.read(cx).value().to_string(),
            suffixes: self.helpers.suffixes.read(cx).value().to_string(),
            capital: self.helpers.capital,
            upper: self.helpers.upper,
        }
    }

    pub(super) fn quick_setup_toggle(&self, cx: &mut Context<Self>) -> gpui_component::button::Button {
        let c = colors(cx);
        let label = match self.attack {
            0 => "快速生成字典/规则",
            1 => "组合快捷操作",
            _ => "常用掩码模板",
        };
        let chevron = if self.helpers.expanded {
            IconName::ChevronUp
        } else {
            IconName::ChevronDown
        };
        choice("toggle-helper", label, self.helpers.expanded, cx)
            .h(px(28.))
            .px(px(10.))
            .gap(px(5.))
            .child(Icon::new(chevron).size(px(13.)).text_color(rgb(c.muted)))
            .on_click(cx.listener(|this, _, _, cx| {
                this.helpers.expanded = !this.helpers.expanded;
                cx.notify();
            }))
    }

    pub(super) fn quick_setup_body(&self, cx: &mut Context<Self>) -> Option<Div> {
        if !self.helpers.expanded {
            return None;
        }
        let c = colors(cx);
        let locked = self.form_locked() || self.picking;
        let mut inner = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .p(px(12.))
            .rounded(px(9.))
            .bg(rgb(c.panel))
            .border_1()
            .border_color(rgb(c.border));
        if self.attack == 0 {
            let generator = self.generator(cx);
            let mut preview = Vec::new();
            let _ = generator.visit(Artifact::Words, |word| {
                preview.push(word.to_owned());
                Ok(preview.len() < 4)
            });
            let mut rules = Vec::new();
            let rule_preview = generator.visit(Artifact::Rules, |line| {
                rules.push(line.to_owned());
                Ok(rules.len() < 5)
            });
            inner = inner
                .child(
                    div()
                        .flex()
                        .gap(px(10.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(multiline(&self.helpers.seeds, 76.).disabled(locked)),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w_0()
                                .justify_between()
                                .gap(px(6.))
                                .child(single_line(&self.helpers.suffixes).disabled(locked))
                                .child(
                                    div().flex().flex_wrap().gap(px(6.)).children(
                                        [
                                            ("capitalize", "首字母大写", false, self.helpers.capital),
                                            ("uppercase", "全大写", true, self.helpers.upper),
                                        ]
                                        .into_iter()
                                        .map(|(id, text, upper, active)| {
                                            choice(id, text, active, cx)
                                                .h(px(30.))
                                                .px(px(10.))
                                                .disabled(locked)
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    if upper {
                                                        this.helpers.upper = !this.helpers.upper;
                                                    } else {
                                                        this.helpers.capital =
                                                            !this.helpers.capital;
                                                    }
                                                    cx.notify();
                                                }))
                                        }),
                                    ),
                                ),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .justify_between()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .min_w_0()
                                .flex_1()
                                .gap(px(3.))
                                .when(!preview.is_empty(), |this| {
                                    this.child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(6.))
                                            .text_size(px(12.))
                                            .text_color(rgb(c.muted))
                                            .child(
                                                div()
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(rgb(c.ink))
                                                    .child("字典"),
                                            )
                                            .child(
                                                div()
                                                    .font_family("Cascadia Mono")
                                                    .truncate()
                                                    .child(preview.join(" · ")),
                                            ),
                                    )
                                })
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(6.))
                                        .text_size(px(12.))
                                        .text_color(rgb(c.muted))
                                        .child(
                                            div()
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(rgb(c.ink))
                                                .child("规则"),
                                        )
                                        .child(
                                            div()
                                                .font_family("Cascadia Mono")
                                                .truncate()
                                                .child(
                                                    rule_preview
                                                        .map(|_| rules.join(" · "))
                                                        .unwrap_or_else(|e| e),
                                                ),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_shrink_0()
                                .gap(px(6.))
                                .children(
                                    [
                                        ("save-words", "保存字典并使用", Artifact::Words),
                                        ("save-rules", "保存规则并使用", Artifact::Rules),
                                    ]
                                    .into_iter()
                                    .map(|(id, text, kind)| {
                                        button(id, cx)
                                            .h(px(30.))
                                            .px(px(10.))
                                            .label(text)
                                            .disabled(locked)
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.save_generated(kind, window, cx)
                                            }))
                                    }),
                                ),
                        ),
                );
        } else if self.attack == 1 {
            let a = self
                .words
                .as_deref()
                .map(crate::hashcat::file_name)
                .unwrap_or_else(|| "第一份字典".into());
            let b = self
                .second_words
                .as_deref()
                .map(crate::hashcat::file_name)
                .unwrap_or_else(|| "第二份字典".into());
            inner = inner
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(rgb(c.muted))
                        .child(format!("组合顺序：{a}  +  {b}")),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(6.))
                        .child(
                            button("pair-words", cx)
                                .h(px(30.))
                                .px(px(10.))
                                .label("同时选择两份")
                                .disabled(locked)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.pick_word_pair(window, cx)
                                })),
                        )
                        .child(
                            button("swap-words", cx)
                                .h(px(30.))
                                .px(px(10.))
                                .label("交换两份字典")
                                .disabled(
                                    locked || (self.words.is_none() && self.second_words.is_none()),
                                )
                                .on_click(cx.listener(|this, _, _, cx| {
                                    std::mem::swap(&mut this.words, &mut this.second_words);
                                    cx.notify();
                                })),
                        )
                        .child(
                            button("same-words", cx)
                                .h(px(30.))
                                .px(px(10.))
                                .label("两边使用第一份")
                                .disabled(locked || self.words.is_none())
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.second_words = this.words.clone();
                                    cx.notify();
                                })),
                        ),
                );
        } else {
            let mut presets: Vec<(&str, &str, u32)> = match self.attack {
                3 => vec![
                    ("4 位数字", "?d?d?d?d", 3),
                    ("6 位数字", "?d?d?d?d?d?d", 3),
                    ("8 位数字", "?d?d?d?d?d?d?d?d", 3),
                    ("1 字母 + 4 数字", "?l?d?d?d?d", 3),
                    ("2 字母 + 4 数字", "?l?l?d?d?d?d", 3),
                ],
                12 => vec![
                    ("字典居中", "?d?w?d", 12),
                    ("连接两个词", "?w-?q", 12),
                    ("双词 + 2 数字", "?w?q?d?d", 12),
                ],
                _ => vec![
                    ("后加 2 位数字", "?d?d", 6),
                    ("后加 4 位数字", "?d?d?d?d", 6),
                    ("前加 4 位数字", "?d?d?d?d", 7),
                    ("后加 1 符号", "?s", 6),
                ],
            };
            inner = inner.child(
                div().flex().flex_wrap().gap(px(6.)).children(
                    presets
                        .drain(..)
                        .enumerate()
                        .map(|(i, (title, mask, mode))| {
                            button(("mask-template", i), cx)
                                .h(px(30.))
                                .px(px(10.))
                                .label(title)
                                .tooltip(format!("套用掩码 {mask}"))
                                .disabled(locked)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    if this.attack != mode {
                                        this.activate_attack(mode, window, cx);
                                    }
                                    this.mask_file = None;
                                    this.mask_text
                                        .update(cx, |s, cx| s.set_value(mask, window, cx));
                                    this.form_error = None;
                                    cx.notify();
                                }))
                        }),
                ),
            );
        }
        let inner = inner.when(!self.helpers.note.is_empty() && self.attack == 0, |box_| {
            box_.child(
                div()
                    .text_size(px(12.))
                    .text_color(rgb(c.success))
                    .child(self.helpers.note.clone()),
            )
        });
        Some(inner)
    }

    fn pick_word_pair(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.form_locked() || self.picking {
            return;
        }
        self.picking = true;
        let picker = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("选择两份字典".into()),
        });
        cx.spawn_in(window, async move |weak, cx| {
            let selected = picker.await;
            let _ = weak.update_in(cx, |this, _, cx| {
                this.picking = false;
                match selected {
                    Ok(Ok(Some(paths))) => {
                        if let [left, right] = paths.as_slice() {
                            this.words = Some(left.clone());
                            this.second_words = Some(right.clone());
                            this.form_error = None;
                        } else {
                            this.form_error = Some("请同时选中两份字典。".into());
                        }
                    }
                    Ok(Ok(None)) => {}
                    _ => this.form_error = Some("无法打开字典选择器。".into()),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn save_generated(&mut self, kind: Artifact, window: &mut Window, cx: &mut Context<Self>) {
        if self.picking || self.form_locked() {
            return;
        }
        let generator = self.generator(cx);
        if let Err(e) = generator.visit(kind, |_| Ok(false)) {
            self.helpers.note = e;
            cx.notify();
            return;
        }
        let dir = self.data.join("dictionaries");
        if let Err(e) = fs::create_dir_all(&dir) {
            self.helpers.note = e.to_string();
            cx.notify();
            return;
        }
        let name = match kind {
            Artifact::Words => "lovelycat-words.txt",
            Artifact::Rules => "lovelycat.rule",
        };
        self.picking = true;
        let picker = cx.prompt_for_new_path(&dir, Some(name));
        cx.spawn_in(window, async move |weak, cx| {
            let selected = picker.await;
            let result = match selected {
                Ok(Ok(Some(path))) => Some(
                    cx.background_spawn(async move {
                        generator.save(kind, &path)?;
                        Ok::<_, String>(path)
                    })
                    .await,
                ),
                Ok(Ok(None)) => None,
                _ => Some(Err("无法打开保存对话框。".into())),
            };
            let _ = weak.update_in(cx, |this, _, cx| {
                this.picking = false;
                match result {
                    Some(Ok(path)) => {
                        this.helpers.note =
                            format!("已保存并选用 {}", crate::hashcat::file_name(&path));
                        match kind {
                            Artifact::Words => this.words = Some(path),
                            Artifact::Rules => this.rule = Some(path),
                        }
                    }
                    Some(Err(e)) => this.helpers.note = e,
                    None => {}
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generation_has_stable_unique_words_and_matching_ascii_rules() {
        let draft = Generator {
            seeds: "cat\ncat".into(),
            suffixes: "47,47".into(),
            capital: true,
            upper: false,
        };
        let mut words = Vec::new();
        draft
            .visit(Artifact::Words, |w| {
                words.push(w.to_string());
                Ok(true)
            })
            .unwrap();
        assert_eq!(words, ["cat", "cat47", "Cat", "Cat47"]);
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("word rules.rule");
        draft.save(Artifact::Rules, &file).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), ":\n$4$7\nc\nc$4$7\n");
        let invalid = Generator {
            suffixes: "猫".into(),
            ..draft
        };
        assert!(invalid.save(Artifact::Rules, &file).is_err());
        assert_eq!(fs::read_to_string(&file).unwrap(), ":\n$4$7\nc\nc$4$7\n");
    }

    #[test]
    #[ignore = "validates the generated rule file with a local Hashcat installation"]
    fn generated_rules_produce_the_previewed_candidates_in_hashcat() {
        use crate::hashcat::{self, CommandEvent};
        let directory = tempfile::tempdir().unwrap();
        let exe = hashcat::locate(directory.path()).expect("test requires Hashcat");
        let words = directory.path().join("base words.txt");
        let rules = directory.path().join("sample.rule");
        fs::write(&words, "cat\n").unwrap();
        let settings = Generator {
            seeds: "cat".into(),
            suffixes: "47".into(),
            capital: true,
            upper: true,
        };
        settings.save(Artifact::Rules, &rules).unwrap();
        let mut expected = Vec::new();
        settings
            .visit(Artifact::Words, |word| {
                expected.push(word.to_string());
                Ok(true)
            })
            .unwrap();
        let args = vec![
            "--stdout".into(),
            "--quiet".into(),
            "--restore-disable".into(),
            "--logfile-disable".into(),
            "-r".into(),
            rules.into_os_string(),
            "--".into(),
            words.into_os_string(),
        ];
        let (_cancel, events) = hashcat::start_command(exe, args, false, None, None);
        let mut actual = Vec::new();
        while let Ok(event) = events.recv_blocking() {
            match event {
                CommandEvent::Line(line) => actual.push(line),
                CommandEvent::Finished(result) => assert_eq!(result.unwrap(), Some(0)),
            }
        }
        expected.sort();
        actual.sort();
        assert_eq!(actual, expected);
    }
}
