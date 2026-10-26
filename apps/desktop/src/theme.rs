use crate::app::Message;
use crate::motion_renderer::Element;
use iced::widget::{button, container, text};
use iced::{border, Background, Border, Color, Font, Theme};

pub const GREEN: Color = Color::from_rgb(0.22, 0.79, 0.56);
pub const WHITE: Color = Color::from_rgb(0.89, 0.92, 0.90);
pub const MUTED: Color = Color::from_rgb(0.51, 0.56, 0.53);
pub const PURPLE: Color = GREEN;
pub const LINE: Color = Color::from_rgb(0.16, 0.19, 0.17);
pub const PANEL: Color = Color::from_rgb(0.075, 0.09, 0.08);
pub const BACKGROUND: Color = Color::from_rgb(0.065, 0.078, 0.070);
pub const SIDEBAR: Color = Color::from_rgb(0.048, 0.059, 0.052);
pub const FONT: Font = Font::with_name("Microsoft YaHei");

pub fn label<'a>(
    value: impl iced::widget::text::IntoFragment<'a>,
    size: f32,
) -> iced::widget::Text<'a, iced::Theme, crate::motion_renderer::Renderer> {
    text(value)
        .size(size)
        .color(WHITE)
        .shaping(text::Shaping::Advanced)
}

pub fn bold<'a>(
    value: impl iced::widget::text::IntoFragment<'a>,
    size: f32,
) -> iced::widget::Text<'a, iced::Theme, crate::motion_renderer::Renderer> {
    label(value, size).font(Font {
        weight: iced::font::Weight::Bold,
        ..FONT
    })
}

pub fn panel(_: &Theme) -> container::Style {
    container::Style {
        background: Some(PANEL.into()),
        border: Border {
            color: LINE,
            width: 1.,
            radius: 10.into(),
        },
        text_color: Some(WHITE),
        ..Default::default()
    }
}

pub fn surface(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgb(0.10, 0.12, 0.105).into()),
        border: Border {
            color: LINE,
            width: 1.,
            radius: 8.into(),
        },
        ..Default::default()
    }
}

pub fn action(_: &Theme, status: button::Status) -> button::Style {
    let disabled = matches!(status, button::Status::Disabled);
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: Some(Background::Color(if disabled {
            Color::from_rgb(0.12, 0.22, 0.17)
        } else if hovered {
            Color::from_rgb(0.29, 0.87, 0.64)
        } else {
            GREEN
        })),
        text_color: if disabled {
            MUTED
        } else {
            Color::from_rgb(0.025, 0.09, 0.055)
        },
        border: border::rounded(9),
        ..Default::default()
    }
}

pub fn quiet(_: &Theme, status: button::Status) -> button::Style {
    button::Style {
        background: matches!(status, button::Status::Hovered | button::Status::Pressed)
            .then_some(Color::from_rgb(0.12, 0.15, 0.13).into()),
        text_color: if matches!(status, button::Status::Disabled) {
            MUTED
        } else {
            WHITE
        },
        border: border::rounded(7),
        ..Default::default()
    }
}

pub fn selected(_: &Theme, _: button::Status) -> button::Style {
    button::Style {
        background: Some(Color::from_rgb(0.11, 0.17, 0.13).into()),
        border: border::rounded(7),
        text_color: WHITE,
        ..Default::default()
    }
}

pub fn input(
    theme: &Theme,
    status: iced::widget::text_input::Status,
) -> iced::widget::text_input::Style {
    let mut style = iced::widget::text_input::default(theme, status);
    style.background = Color::from_rgb(0.09, 0.11, 0.095).into();
    style.border = Border {
        color: LINE,
        width: 1.,
        radius: 8.into(),
    };
    style.value = WHITE;
    style.placeholder = MUTED;
    style
}

pub fn composer(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgb(0.105, 0.13, 0.113).into()),
        border: Border {
            color: Color::from_rgb(0.20, 0.25, 0.215),
            width: 1.,
            radius: 16.into(),
        },
        ..Default::default()
    }
}

pub fn pill(value: impl Into<String>, active: bool) -> Element<'static, Message> {
    container(label(value.into(), 11.).color(if active { GREEN } else { MUTED }))
        .padding([4, 9])
        .style(move |_| container::Style {
            background: Some(if active {
                Color::from_rgba(0., 0.6, 0.4, 0.14).into()
            } else {
                Color::from_rgb(0.12, 0.15, 0.13).into()
            }),
            border: border::rounded(16),
            ..Default::default()
        })
        .into()
}

/// Reserve a narrow gutter so scrollbars never cover buttons or text.
pub fn scroll_direction() -> iced::widget::scrollable::Direction {
    iced::widget::scrollable::Direction::Vertical(
        iced::widget::scrollable::Scrollbar::new()
            .width(4)
            .scroller_width(4)
            .spacing(8),
    )
}

pub fn launcher() -> Theme {
    Theme::custom(
        "Memoria",
        iced::theme::Palette {
            background: BACKGROUND,
            text: WHITE,
            primary: GREEN,
            success: GREEN,
            warning: Color::from_rgb(1., 0.75, 0.25),
            danger: Color::from_rgb(0.95, 0.25, 0.35),
        },
    )
}
