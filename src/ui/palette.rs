use gpui::{App, Global, px, rgb};
use gpui_component::{Theme, ThemeMode};

use crate::desktop::Appearance;

#[derive(Clone, Copy)]
pub(super) struct Palette {
    pub background: u32,
    pub panel: u32,
    pub monitor: u32,
    pub raised: u32,
    pub border: u32,
    pub ink: u32,
    pub muted: u32,
    pub accent: u32,
    pub accent_ink: u32,
    pub accent_hover: u32,
    pub selection: u32,
    pub selection_border: u32,
    pub success: u32,
    pub error: u32,
    pub error_fill: u32,
}

impl Global for Palette {}

pub(super) fn colors(cx: &App) -> Palette {
    *cx.global::<Palette>()
}

pub fn configure_theme(appearance: Appearance, cx: &mut App) {
    let (mode, colors) = match appearance {
        Appearance::Light => (
            ThemeMode::Light,
            Palette {
                background: 0xf5f4ef,
                panel: 0xfffefb,
                monitor: 0xfbfaf6,
                raised: 0xefede6,
                border: 0xe2ded4,
                ink: 0x24231f,
                muted: 0x605d56,
                accent: 0xefc96f,
                accent_ink: 0x312716,
                accent_hover: 0xe9ba58,
                selection: 0xfcf1d5,
                selection_border: 0xdcb86a,
                success: 0x296346,
                error: 0x9b3532,
                error_fill: 0xfcf0ed,
            },
        ),
        Appearance::Dark => (
            ThemeMode::Dark,
            Palette {
                background: 0x191a1d,
                panel: 0x222326,
                monitor: 0x1d1e21,
                raised: 0x2d2e33,
                border: 0x3c3d43,
                ink: 0xf0eeea,
                muted: 0xbebdc1,
                accent: 0xeec86e,
                accent_ink: 0x312716,
                accent_hover: 0xf6d98d,
                selection: 0x403623,
                selection_border: 0x8b6c39,
                success: 0xa6d3ad,
                error: 0xf2b0ab,
                error_fill: 0x432c2b,
            },
        ),
        Appearance::Mint => (
            ThemeMode::Light,
            Palette {
                background: 0xecf4ef,
                panel: 0xf9fdf9,
                monitor: 0xf1f7f4,
                raised: 0xe2eee6,
                border: 0xc9dbd1,
                ink: 0x20352c,
                muted: 0x4d6559,
                accent: 0x9bd7b6,
                accent_ink: 0x173529,
                accent_hover: 0x84caa3,
                selection: 0xdcf2e4,
                selection_border: 0x76a88c,
                success: 0x27603e,
                error: 0x933536,
                error_fill: 0xffefeb,
            },
        ),
        Appearance::Midnight => (
            ThemeMode::Dark,
            Palette {
                background: 0x141c2a,
                panel: 0x1c2738,
                monitor: 0x192332,
                raised: 0x29364a,
                border: 0x394961,
                ink: 0xeaf1fb,
                muted: 0xb1bfd4,
                accent: 0xa2bff0,
                accent_ink: 0x142945,
                accent_hover: 0xbdd5ff,
                selection: 0x2b3f5b,
                selection_border: 0x657fa8,
                success: 0x9ddbc4,
                error: 0xffafae,
                error_fill: 0x442e39,
            },
        ),
    };
    Theme::change(mode, None, cx);
    let theme = Theme::global_mut(cx);
    theme.font_family = "Segoe UI".into();
    theme.font_size = px(16.);
    theme.mono_font_family = "Cascadia Mono".into();
    theme.radius = px(8.);
    // Root paints this beneath our antialiased desktop surface.
    theme.background = gpui::transparent_black();
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
