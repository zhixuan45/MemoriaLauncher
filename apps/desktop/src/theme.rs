use crate::app::Message;
use iced::widget::{button, container, text};
use iced::{border, Background, Border, Color, Element, Font, Theme};

pub const GREEN: Color = Color::from_rgb(0., 0.91, 0.67);
pub const WHITE: Color = Color::from_rgb(0.90, 0.94, 0.97);
pub const MUTED: Color = Color::from_rgb(0.52, 0.62, 0.68);
pub const PURPLE: Color = Color::from_rgb(0.73, 0.47, 0.94);
pub const LINE: Color = Color::from_rgba(0.31, 0.48, 0.55, 0.23);
pub const PANEL: Color = Color::from_rgba(0.025, 0.075, 0.10, 0.94);
pub const FONT: Font = Font::with_name("Microsoft YaHei");

pub fn label<'a>(
    value: impl iced::widget::text::IntoFragment<'a>,
    size: f32,
) -> iced::widget::Text<'a> {
    text(value)
        .size(size)
        .color(WHITE)
        .shaping(text::Shaping::Advanced)
}

pub fn bold<'a>(
    value: impl iced::widget::text::IntoFragment<'a>,
    size: f32,
) -> iced::widget::Text<'a> {
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
        background: Some(Color::from_rgba(0.035, 0.095, 0.12, 0.68).into()),
        border: Border {
            color: LINE,
            width: 1.,
            radius: 8.into(),
        },
        ..Default::default()
    }
}

pub fn action(_: &Theme, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: Some(Background::Color(if hovered {
            Color::from_rgb(0.22, 1., 0.77)
        } else {
            GREEN
        })),
        text_color: Color::from_rgb(0.01, 0.10, 0.09),
        border: border::rounded(9),
        ..Default::default()
    }
}

pub fn quiet(_: &Theme, status: button::Status) -> button::Style {
    button::Style {
        background: Some(
            if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                Color::from_rgba(0.2, 0.37, 0.43, 0.55).into()
            } else {
                Color::from_rgba(0.03, 0.09, 0.12, 0.7).into()
            },
        ),
        text_color: WHITE,
        border: Border {
            color: LINE,
            width: 1.,
            radius: 8.into(),
        },
        ..Default::default()
    }
}

pub fn selected(_: &Theme, _: button::Status) -> button::Style {
    button::Style {
        background: Some(Color::from_rgba(0., 0.39, 0.29, 0.60).into()),
        border: Border {
            color: GREEN,
            width: 1.,
            radius: 9.into(),
        },
        text_color: WHITE,
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
                Color::from_rgba(0.1, 0.16, 0.2, 0.65).into()
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
            background: Color::from_rgb(0.025, 0.065, 0.085),
            text: WHITE,
            primary: GREEN,
            success: GREEN,
            warning: Color::from_rgb(1., 0.75, 0.25),
            danger: Color::from_rgb(0.95, 0.25, 0.35),
        },
    )
}
