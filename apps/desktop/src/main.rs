mod app;
mod assets;
mod home;
mod models;
mod settings;
mod shell;
mod theme;
mod verification;

use app::Launcher;

fn main() -> iced::Result {
    let size = verification::window_size();
    let mut application = iced::application(Launcher::new, Launcher::update, Launcher::view)
        .title("MC 启动器")
        .theme(iced::Theme::Dark)
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
    for path in ["C:/Windows/Fonts/msyh.ttc", "C:/Windows/Fonts/msyhbd.ttc"] {
        if let Ok(bytes) = std::fs::read(path) {
            application = application.font(bytes);
        }
    }
    application.run()
}
