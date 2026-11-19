#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod desktop;
mod downloads;
mod forensics;
mod hashcat;
mod process;
mod protected;
mod ui;
mod updater;

use gpui::{
    AppContext, Application, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size,
};
use gpui_component::Root;
use gpui_component_assets::Assets;

fn main() {
    let data = hashcat::data_dir();
    let preferences = desktop::Preferences::load(&data);
    Application::new().with_assets(Assets).run(move |cx| {
        gpui_component::init(cx);
        ui::configure_theme(preferences.appearance, cx);
        let available = cx
            .primary_display()
            .map(|display| display.bounds().size)
            .unwrap_or(size(px(1280.), px(900.)));
        let bounds = Bounds::centered(
            None,
            size(
                px(1200.).min(available.width - px(32.)),
                px(800.).min(available.height - px(80.)),
            ),
            cx,
        );
        let opened = cx.open_window(
            WindowOptions {
                app_id: Some("Lovelycat".into()),
                titlebar: Some(TitlebarOptions {
                    title: Some("Lovelycat".into()),
                    appears_transparent: cfg!(windows),
                    traffic_light_position: None,
                }),
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_background: if cfg!(windows) {
                    gpui::WindowBackgroundAppearance::Transparent
                } else {
                    gpui::WindowBackgroundAppearance::Opaque
                },
                window_min_size: Some(size(px(720.), px(520.))),
                ..Default::default()
            },
            |window, cx| {
                let workbench = cx.new(|cx| ui::Workbench::new(data, preferences, window, cx));
                cx.new(|cx| Root::new(workbench, window, cx))
            },
        );
        if let Err(error) = opened {
            eprintln!("Unable to open Lovelycat: {error}");
            cx.quit();
        }
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
}
