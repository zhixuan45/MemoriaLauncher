use iced::widget::{image, svg};
use iced::{Color, ContentFit};
use std::sync::LazyLock;

pub struct Assets {
    pub wallpaper: image::Handle,
    pub sidebar: image::Handle,
    pub grass: image::Handle,
    pub anvil: image::Handle,
    pub modpack: image::Handle,
    pub sodium: image::Handle,
    pub banners: [image::Handle; 4],
}

pub static ART: LazyLock<Assets> = LazyLock::new(|| Assets {
    wallpaper: image::Handle::from_bytes(
        include_bytes!("../ui/assets/bg_wallpaper.png").as_slice(),
    ),
    sidebar: image::Handle::from_bytes(
        include_bytes!("../ui/assets/bg_sidebar_bottom.png").as_slice(),
    ),
    grass: image::Handle::from_bytes(include_bytes!("../ui/assets/icon_grass.png").as_slice()),
    anvil: image::Handle::from_bytes(include_bytes!("../ui/assets/icon_anvil.png").as_slice()),
    modpack: image::Handle::from_bytes(include_bytes!("../ui/assets/icon_modpack.png").as_slice()),
    sodium: image::Handle::from_bytes(include_bytes!("../ui/assets/icon_sodium.png").as_slice()),
    banners: [
        image::Handle::from_bytes(include_bytes!("../ui/assets/banner_update.png").as_slice()),
        image::Handle::from_bytes(include_bytes!("../ui/assets/banner_rpg.png").as_slice()),
        image::Handle::from_bytes(include_bytes!("../ui/assets/banner_cherry.png").as_slice()),
        image::Handle::from_bytes(include_bytes!("../ui/assets/banner_update.png").as_slice()),
    ],
});

pub fn picture(handle: &image::Handle, size: f32) -> iced::widget::Image {
    image(handle.clone())
        .width(size)
        .height(size)
        .content_fit(ContentFit::Contain)
}

#[derive(Debug, Clone, Copy)]
pub enum Icon {
    Home,
    Download,
    Spark,
    Settings,
    Play,
    Sliders,
    Cube,
    Folder,
    Gamepad,
    Left,
    Right,
    Pause,
    Bell,
    Minimize,
    Maximize,
    Close,
    Edit,
    Info,
}

pub fn icon(kind: Icon, color: Color, size: f32) -> iced::widget::Svg<'static> {
    let path = match kind {
        Icon::Home => "M3 11 12 3l9 8M5 10v11h5v-7h4v7h5V10",
        Icon::Download => "M12 3v12m-5-5 5 5 5-5M4 16v5h16v-5",
        Icon::Spark => "m12 2 3 7 7 3-7 3-3 7-3-7-7-3 7-3ZM21 2v4m-2-2h4",
        Icon::Settings => "m9 3-1 3-3 1-2 4 2 2v4l3 1 2 3h4l2-3 3-1v-4l2-2-2-4-3-1-1-3ZM12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8",
        Icon::Play => "m8 4 12 8-12 8Z",
        Icon::Sliders => "M3 6h18M3 12h18M3 18h18M8 3v6m8 0v6m-7 0v6",
        Icon::Cube => "m12 2 10 5v10l-10 5-10-5V7Zm0 10v10M2 7l10 5L22 7M7 4l10 5",
        Icon::Folder => "M2 7V4h7l3 3h10v14H2Z",
        Icon::Gamepad => "M7 6h10c4 0 6 14 3 14l-5-5H9l-5 5C1 20 3 6 7 6ZM6 10h6m-3-3v6m7-3h.1m3 3h.1",
        Icon::Left => "m15 5-7 7 7 7",
        Icon::Right => "m9 5 7 7-7 7",
        Icon::Pause => "M8 5v14m8-14v14",
        Icon::Bell => "M5 17h14l-2-4V8a5 5 0 0 0-10 0v5ZM10 21h4M12 1v2",
        Icon::Minimize => "M5 12h14",
        Icon::Maximize => "M5 5h14v14H5Z",
        Icon::Close => "m6 6 12 12M6 18 18 6",
        Icon::Edit => "M3 3h7v7H3Zm11 0h7v7h-7ZM3 14h7v7H3Zm11 0h7v7h-7Z",
        Icon::Info => "M12 8h.1m-.1 4v5M22 12a10 10 0 1 0-20 0 10 10 0 0 0 20 0",
    };
    let color = format!(
        "rgb({},{},{})",
        (color.r * 255.) as u8,
        (color.g * 255.) as u8,
        (color.b * 255.) as u8
    );
    let fill = if matches!(kind, Icon::Play | Icon::Spark) {
        color.as_str()
    } else {
        "none"
    };
    let xml = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="{path}" fill="{fill}" stroke="{color}" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"/></svg>"#
    );
    svg(svg::Handle::from_memory(xml.into_bytes()))
        .width(size)
        .height(size)
}
