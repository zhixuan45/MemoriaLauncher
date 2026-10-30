// Windows release 构建下隐藏控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod accounts;
mod accounts_flow;
mod ai;
mod ai_settings;
mod api_client;
mod app;
mod appearance;
mod instance_settings;
mod resource_install;
mod assets;
mod attachments;
mod attachments_view;
#[cfg(test)]
mod chat_tests;
mod chat_view;
mod credentials;
mod dashboard;
mod download;
mod download_settings;
mod game_settings;
mod general;
mod home;
#[cfg(test)]
mod interaction_tests;
mod java_environment;
mod local;
mod loader_install;
mod diagnostics;
mod dependency_view;
mod mcp_settings;
mod models;
mod motion;
mod motion_renderer;
mod resources;
mod runtime_memory;
mod settings;
mod selections;
mod change_plans;
mod instance_wizard;
mod pack_export;
mod shell;
mod sidebar;
mod project_menu;
mod project_preferences;
mod theme;
mod updates;
mod verification;
mod workspace;

use app::Launcher;

fn main() -> iced::Result {
    mc_core::initialize_runtime();
    let size = verification::window_size();
    let application = iced::application(Launcher::new, Launcher::update, Launcher::view)
        .title("MC 启动器")
        .theme(Launcher::theme)
        .scale_factor(Launcher::scale_factor)
        .default_font(theme::FONT)
        .subscription(Launcher::subscription)
        .window(iced::window::Settings {
            size,
            min_size: Some(iced::Size::new(1024., 680.)),
            position: iced::window::Position::Centered,
            decorations: false,
            exit_on_close_request: false,
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
