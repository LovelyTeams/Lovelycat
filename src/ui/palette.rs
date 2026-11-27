use gpui::{App, Global, px, rgb};
use gpui_component::{Theme, ThemeMode};

use crate::desktop::Appearance;

#[derive(Clone, Copy)]
pub(super) struct Palette {
    pub background: u32,
    pub panel: u32,
    pub monitor: u32,
    pub raised: u32,
    pub hover: u32,
    pub border: u32,
    pub subtle_border: u32,
    pub ink: u32,
    pub muted: u32,
    pub accent: u32,
    pub accent_ink: u32,
    pub accent_hover: u32,
    pub selection: u32,
    pub selection_hover: u32,
    pub selection_border: u32,
    pub success: u32,
    pub success_fill: u32,
    pub error: u32,
    pub error_fill: u32,
}

impl Global for Palette {}

pub(super) fn colors(cx: &App) -> Palette {
    *cx.global::<Palette>()
}

pub(super) fn theme_swatch(appearance: Appearance) -> (u32, u32, u32) {
    match appearance {
        Appearance::Light => (0xf5f4ef, 0xfffefb, 0xefc96f),
        Appearance::Mint => (0xecf4ef, 0xf9fdf9, 0x84caa3),
        Appearance::Dark => (0x191a1d, 0x222326, 0xeec86e),
        Appearance::Midnight => (0x141c2a, 0x1c2738, 0xa2bff0),
    }
}

pub(super) fn theme_name(appearance: Appearance) -> &'static str {
    match appearance {
        Appearance::Light => "暖白",
        Appearance::Mint => "薄荷",
        Appearance::Dark => "炭黑",
        Appearance::Midnight => "午夜蓝",
    }
}

pub fn configure_theme(appearance: Appearance, cx: &mut App) {
    let (mode, colors) = match appearance {
        Appearance::Light => (
            ThemeMode::Light,
            Palette {
                background: 0xf4f2ec,
                panel: 0xfffefb,
                monitor: 0xfaf9f5,
                raised: 0xeeece4,
                hover: 0xe6e3d9,
                border: 0xe0dcd1,
                subtle_border: 0xebe8df,
                ink: 0x22211d,
                muted: 0x625f57,
                accent: 0xefc96f,
                accent_ink: 0x312716,
                accent_hover: 0xe7b74f,
                selection: 0xfcf1d4,
                selection_hover: 0xf7e7be,
                selection_border: 0xd9b35f,
                success: 0x256142,
                success_fill: 0xe7f4ec,
                error: 0x9b3532,
                error_fill: 0xfceee9,
            },
        ),
        Appearance::Dark => (
            ThemeMode::Dark,
            Palette {
                background: 0x17181b,
                panel: 0x202125,
                monitor: 0x1b1c20,
                raised: 0x2a2c31,
                hover: 0x33353c,
                border: 0x383a41,
                subtle_border: 0x2c2e34,
                ink: 0xf1efeb,
                muted: 0xb2b1b7,
                accent: 0xeec86e,
                accent_ink: 0x2b2212,
                accent_hover: 0xf5d687,
                selection: 0x3c3220,
                selection_hover: 0x483c26,
                selection_border: 0x8f6f39,
                success: 0x98d4a3,
                success_fill: 0x213628,
                error: 0xf2ada8,
                error_fill: 0x422a29,
            },
        ),
        Appearance::Mint => (
            ThemeMode::Light,
            Palette {
                background: 0xebf3ee,
                panel: 0xf9fdf9,
                monitor: 0xf1f7f4,
                raised: 0xe1ede5,
                hover: 0xd6e6dc,
                border: 0xc6dad0,
                subtle_border: 0xdbe9e1,
                ink: 0x1e332a,
                muted: 0x4e675b,
                accent: 0x96d5b2,
                accent_ink: 0x153326,
                accent_hover: 0x80c8a0,
                selection: 0xdaf1e3,
                selection_hover: 0xccead8,
                selection_border: 0x71a688,
                success: 0x245e3c,
                success_fill: 0xe2f3e8,
                error: 0x933536,
                error_fill: 0xffefeb,
            },
        ),
        Appearance::Midnight => (
            ThemeMode::Dark,
            Palette {
                background: 0x121a27,
                panel: 0x1a2536,
                monitor: 0x16202f,
                raised: 0x253347,
                hover: 0x2e3f57,
                border: 0x35465e,
                subtle_border: 0x26354a,
                ink: 0xeaf1fb,
                muted: 0xaab9cf,
                accent: 0xa2bff0,
                accent_ink: 0x132742,
                accent_hover: 0xb9d2fc,
                selection: 0x283c57,
                selection_hover: 0x314969,
                selection_border: 0x657fa8,
                success: 0x99dac2,
                success_fill: 0x1c3534,
                error: 0xffadaa,
                error_fill: 0x422b36,
            },
        ),
    };
    Theme::change(mode, None, cx);
    let theme = Theme::global_mut(cx);
    theme.font_family = "Segoe UI".into();
    theme.font_size = px(16.);
    theme.mono_font_family = "Cascadia Mono".into();
    theme.radius = px(8.);
    // Root paints this beneath our antialiased desktop surface on Windows.
    // Workbench switches Theme.background to an opaque panel color for child widgets
    // (Select dropdowns, Inputs, Tooltips) and restores transparency on Windows after prepaint.
    theme.background = if cfg!(windows) {
        gpui::transparent_black()
    } else {
        rgb(colors.panel).into()
    };
    theme.foreground = rgb(colors.ink).into();
    theme.muted = rgb(colors.raised).into();
    theme.secondary = rgb(colors.raised).into();
    theme.accent = rgb(colors.selection).into();
    theme.accent_foreground = rgb(colors.ink).into();
    theme.caret = rgb(colors.ink).into();
    theme.list_head = rgb(colors.monitor).into();
    theme.scrollbar_thumb = rgb(colors.selection_border).into();
    theme.primary = rgb(colors.accent).into();
    theme.primary_hover = rgb(colors.accent_hover).into();
    theme.primary_foreground = rgb(colors.accent_ink).into();
    theme.border = rgb(colors.border).into();
    theme.input = rgb(colors.border).into();
    theme.ring = rgb(colors.selection_border).into();
    theme.muted_foreground = rgb(colors.muted).into();
    theme.popover = rgb(colors.panel).into();
    theme.popover_foreground = rgb(colors.ink).into();
    theme.list = rgb(colors.panel).into();
    theme.list_hover = rgb(colors.raised).into();
    theme.list_active = rgb(colors.selection).into();
    cx.set_global(colors);
}

pub(super) fn set_widget_background(colors: Palette, cx: &mut App) {
    Theme::global_mut(cx).background = rgb(colors.panel).into();
}

#[cfg(windows)]
pub(super) fn reset_root_background(cx: &mut App) {
    Theme::global_mut(cx).background = gpui::transparent_black();
}
