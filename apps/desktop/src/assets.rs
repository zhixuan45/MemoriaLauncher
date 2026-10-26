use iced::widget::{image, svg};
use iced::{Color, ContentFit};
use std::{cell::RefCell, collections::HashMap, sync::LazyLock};

pub struct Assets {
    pub grass: image::Handle,
    pub anvil: image::Handle,
    pub modpack: image::Handle,
    pub sodium: image::Handle,
    pub banners: [image::Handle; 4],
}

pub static ART: LazyLock<Assets> = LazyLock::new(|| Assets {
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

pub fn picture(handle: &image::Handle, size: f32) -> iced::widget::Image<image::Handle> {
    image(handle.clone())
        .width(size)
        .height(size)
        .content_fit(ContentFit::Contain)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Icon {
    Home,
    Plus,
    Search,
    Chat,
    ArrowUp,
    ChevronDown,
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
    Minimize,
    Maximize,
    Close,
    Edit,
    Info,
}

// Stable handles avoid rebuilding and hashing SVG XML on every view update.
// Only the finite palette used by the interface is cached, independently of size.
type IconKey = (Icon, [u8; 4]);
thread_local! {
    static ICONS: RefCell<HashMap<IconKey, svg::Handle>> = RefCell::new(HashMap::new());
}

pub fn icon(kind: Icon, color: Color, size: f32) -> iced::widget::Svg<'static, iced::Theme> {
    let rgba = color.into_rgba8();
    let handle = ICONS.with(|cache| {
        cache
            .borrow_mut()
            .entry((kind, rgba))
            .or_insert_with(|| make_icon(kind, rgba))
            .clone()
    });
    svg(handle).width(size).height(size).style(move |theme: &iced::Theme,_| svg::Style {color:if color==crate::theme::GREEN {Some(theme.palette().primary)}else if color==Color::BLACK {Some(crate::theme::on_accent(theme.palette().primary))}else{None}})
}

fn make_icon(kind: Icon, rgba: [u8; 4]) -> svg::Handle {
    let path = match kind {
        Icon::Plus => "M12 5v14M5 12h14",
        Icon::Search => "M21 21l-5-5M18 10a8 8 0 1 0-16 0 8 8 0 0 0 16 0",
        Icon::Chat => "M3 4h18v13H9l-6 4V4Z",
        Icon::ArrowUp => "M12 20V4m-6 6 6-6 6 6",
        Icon::ChevronDown => "m5 9 7 7 7-7",
        Icon::Home => "M3 11 12 3l9 8M5 10v11h5v-7h4v7h5V10",
        Icon::Download => "M12 3v12m-5-5 5 5 5-5M4 16v5h16v-5",
        Icon::Spark => "m12 2 3 7 7 3-7 3-3 7-3-7-7-3 7-3ZM21 2v4m-2-2h4",
        Icon::Settings => "M9 3h6v3l3 2 3-1 2 5-3 2v3l-4 4-3-1-3 1-4-4v-3L2 12l2-5 3 1 2-2ZM16 12a4 4 0 1 0-8 0 4 4 0 0 0 8 0",
        Icon::Play => "m8 4 12 8-12 8Z",
        Icon::Sliders => "M3 6h18M3 12h18M3 18h18M8 3v6m8 0v6m-7 0v6",
        Icon::Cube => "m12 2 10 5v10l-10 5-10-5V7Zm0 10v10M2 7l10 5L22 7M7 4l10 5",
        Icon::Folder => "M2 7V4h7l3 3h10v14H2Z",
        Icon::Gamepad => {
            "M7 6h10c4 0 6 14 3 14l-5-5H9l-5 5C1 20 3 6 7 6ZM6 10h6m-3-3v6m7-3h.1m3 3h.1"
        }
        Icon::Left => "m15 5-7 7 7 7",
        Icon::Right => "m9 5 7 7-7 7",
        Icon::Pause => "M8 5v14m8-14v14",
        Icon::Minimize => "M5 12h14",
        Icon::Maximize => "M5 5h14v14H5Z",
        Icon::Close => "m6 6 12 12M6 18 18 6",
        Icon::Edit => "M3 3h7v7H3Zm11 0h7v7h-7ZM3 14h7v7H3Zm11 0h7v7h-7Z",
        Icon::Info => "M12 8h.1m-.1 4v5M22 12a10 10 0 1 0-20 0 10 10 0 0 0 20 0",
    };
    let color = format!("rgb({},{},{})", rgba[0], rgba[1], rgba[2]);
    let fill = if matches!(kind, Icon::Play | Icon::Spark) {
        color.as_str()
    } else {
        "none"
    };
    let opacity = rgba[3] as f32 / 255.;
    let xml = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="{path}" fill="{fill}" stroke="{color}" opacity="{opacity}" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"/></svg>"#
    );
    svg::Handle::from_memory(xml.into_bytes())
}
