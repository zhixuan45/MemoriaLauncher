use crate::app::Message;
use crate::motion_renderer::Element;
use iced::widget::{button, container, text};
use iced::{border, Background, Border, Color, Font, Theme};

pub const GREEN: Color = Color::from_rgb(0.22, 0.79, 0.56);
pub const WHITE: Color = Color::from_rgb(0.89, 0.92, 0.90);
pub const MUTED: Color = Color::from_rgb(0.51, 0.56, 0.53);
pub const PURPLE: Color = GREEN;
pub const LINE: Color = Color::from_rgb(0.16, 0.19, 0.17);
pub const BACKGROUND: Color = Color::from_rgb(0.065, 0.078, 0.070);
pub const FONT: Font = Font::with_name("Microsoft YaHei");

pub fn label<'a>(
    value: impl iced::widget::text::IntoFragment<'a>,
    size: f32,
) -> iced::widget::Text<'a, iced::Theme, crate::motion_renderer::Renderer> {
    text(value).size(size).shaping(text::Shaping::Advanced)
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

pub fn shade(theme: &Theme, amount: f32) -> Color {
    let base = theme.palette().background;
    Color::from_rgb(
        (base.r + amount).clamp(0., 1.),
        (base.g + amount).clamp(0., 1.),
        (base.b + amount).clamp(0., 1.),
    )
}
pub fn accent_text(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(theme.palette().primary),
    }
}
pub fn action_text(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(on_accent(theme.palette().primary)),
    }
}
pub fn on_accent(color: Color) -> Color {
    if color.relative_luminance() < 0.32 {
        WHITE
    } else {
        Color::from_rgb(0.025, 0.04, 0.035)
    }
}
pub fn panel(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(shade(theme, 0.01).into()),
        border: Border {
            color: shade(theme, 0.10),
            width: 1.,
            radius: 10.into(),
        },
        text_color: Some(WHITE),
        ..Default::default()
    }
}

pub fn surface(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(shade(theme, 0.04).into()),
        border: Border {
            color: LINE,
            width: 1.,
            radius: 8.into(),
        },
        ..Default::default()
    }
}

pub fn action(theme: &Theme, status: button::Status) -> button::Style {
    let disabled = matches!(status, button::Status::Disabled);
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: Some(Background::Color(if disabled {
            shade(theme, 0.06)
        } else if hovered {
            {
                let color = theme.palette().primary;
                Color::from_rgb(
                    (color.r + 0.08).min(1.),
                    (color.g + 0.08).min(1.),
                    (color.b + 0.08).min(1.),
                )
            }
        } else {
            theme.palette().primary
        })),
        text_color: if disabled {
            MUTED
        } else {
            on_accent(theme.palette().primary)
        },
        border: border::rounded(9),
        ..Default::default()
    }
}

pub fn quiet(theme: &Theme, status: button::Status) -> button::Style {
    button::Style {
        background: matches!(status, button::Status::Hovered | button::Status::Pressed)
            .then_some(shade(theme, 0.07).into()),
        text_color: if matches!(status, button::Status::Disabled) {
            MUTED
        } else {
            WHITE
        },
        border: border::rounded(7),
        ..Default::default()
    }
}

pub fn selected(theme: &Theme, _: button::Status) -> button::Style {
    button::Style {
        background: Some(
            {
                let bg = theme.palette().background;
                let accent = theme.palette().primary;
                Color::from_rgb(
                    bg.r * 0.8 + accent.r * 0.2,
                    bg.g * 0.8 + accent.g * 0.2,
                    bg.b * 0.8 + accent.b * 0.2,
                )
            }
            .into(),
        ),
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
    style.background = shade(theme, 0.03).into();
    style.border = Border {
        color: LINE,
        width: 1.,
        radius: 8.into(),
    };
    style.value = WHITE;
    style.placeholder = MUTED;
    style
}

pub fn composer(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(shade(theme, 0.04).into()),
        border: Border {
            color: Color::from_rgb(0.20, 0.25, 0.215),
            width: 1.,
            radius: 16.into(),
        },
        ..Default::default()
    }
}

pub fn pill(value: impl Into<String>, active: bool) -> Element<'static, Message> {
    container(
        label(value.into(), 11.).style(move |theme: &Theme| text::Style {
            color: Some(if active {
                theme.palette().primary
            } else {
                MUTED
            }),
        }),
    )
    .padding([4, 9])
    .style(move |theme: &Theme| container::Style {
        background: Some(if active {
            Color {
                a: 0.14,
                ..theme.palette().primary
            }
            .into()
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
