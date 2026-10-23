// Windows release 构建下隐藏控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod accounts;
mod accounts_flow;
mod ai;
mod ai_settings;
mod api_client;
mod app;
mod assets;
mod credentials;
mod download;
mod download_settings;
mod game_settings;
mod home;
mod java_environment;
#[cfg(test)]
mod interaction_tests;
mod local;
mod models;
mod motion;
mod motion_renderer;
mod runtime_memory;
mod settings;
mod shell;
mod theme;
mod verification;

use app::Launcher;

fn main() -> iced::Result {
    mc_core::initialize_runtime();
    let size = verification::window_size();
    let application = iced::application(Launcher::new, Launcher::update, Launcher::view)
        .title("MC 启动器")
        .theme(theme::launcher())
        .default_font(theme::FONT)
        .subscription(Launcher::subscription)
        .window(iced::window::Settings {
            size,
            min_size: Some(iced::Size::new(1024., 680.)),
            position: iced::window::Position::Centered,
            decorations: false,
            ..Default::default()
        });
    // Read the platform font at runtime instead of bundling a Windows font.
    #[cfg(target_os = "windows")]
    let application = {
        let mut application = application;
        for path in ["C:/Windows/Fonts/msyh.ttc", "C:/Windows/Fonts/msyhbd.ttc"] {
            if let Ok(bytes) = std::fs::read(path) {
                application = application.font(bytes);
            }
        }
        application
    };
    application.run()
}
