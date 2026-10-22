//! Reusable timed fades and a layout-preserving pop effect.
use crate::motion_renderer::{Element, Renderer};
use iced::advanced::{
    layout, mouse, overlay, renderer, widget::Tree, Clipboard, Layout, Shell, Widget,
};
use iced::{Event, Length, Rectangle, Size, Theme, Transformation, Vector};
use std::time::{Duration, Instant};

pub const PAGE_DURATION: Duration = Duration::from_millis(180);
pub const CARD_DURATION: Duration = Duration::from_millis(50);
pub const SWITCH_DURATION: Duration = Duration::from_millis(120);
pub const OPEN_DURATION: Duration = Duration::from_millis(220);
pub const CLOSE_DURATION: Duration = Duration::from_millis(150);

pub struct Tween {
    from: f32,
    target: f32,
    started: Instant,
    duration: Duration,
}

pub struct Toggle {
    enabled: bool,
    motion: Tween,
}

impl Toggle {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            motion: Tween::new(f32::from(enabled), Instant::now()),
        }
    }

    pub fn set(&mut self, enabled: bool, now: Instant) {
        self.enabled = enabled;
        self.motion.to(f32::from(enabled), SWITCH_DURATION, now);
    }

    pub fn active(&self, now: Instant) -> bool {
        self.motion.active(now)
    }

    pub fn labeled_view<'a, Message: Clone + 'a>(
        &self,
        now: Instant,
        size: f32,
        title: &'a str,
        on_toggle: impl Fn(bool) -> Message,
    ) -> Element<'a, Message> {
        use iced::widget::{mouse_area, row};
        mouse_area(
            row![
                crate::theme::label(title, 11.),
                fade(self.view(now, size, &on_toggle), 1.0, false),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        )
        .on_press(on_toggle(!self.enabled))
        .interaction(mouse::Interaction::Pointer)
        .into()
    }

    pub fn view<'a, Message: Clone + 'a>(
        &self,
        now: Instant,
        size: f32,
        on_toggle: impl Fn(bool) -> Message,
    ) -> Element<'a, Message> {
        use iced::widget::{container, pin, stack, toggler, Space};
        let progress = self.motion.value(now);
        let message = on_toggle(!self.enabled);
        let thumb = switch_thumb(size, progress);
        // Keep native toggler input, but draw its thumb at the interpolated
        // position so reversing a transition never jumps between endpoints.
        stack![
            toggler(self.enabled)
                .size(size)
                .on_toggle(move |_| message.clone())
                .style(move |theme: &Theme, status| {
                    let palette = theme.extended_palette();
                    let mut style = toggler::default(theme, status);
                    style.background = mix(
                        palette.background.strong.color,
                        palette.primary.base.color,
                        progress,
                    )
                    .into();
                    style.foreground = iced::Color::TRANSPARENT.into();
                    style
                }),
            pin(container(Space::new())
                .width(thumb.width)
                .height(thumb.height)
                .style(move |theme: &Theme| {
                    let palette = theme.extended_palette();
                    container::Style {
                        background: Some(
                            mix(
                                palette.background.base.color,
                                palette.primary.base.text,
                                progress,
                            )
                            .into(),
                        ),
                        border: iced::border::rounded(size / 2.0),
                        ..Default::default()
                    }
                }))
            .x(thumb.x)
            .y(thumb.y),
        ]
        .into()
    }
}

fn switch_thumb(size: f32, progress: f32) -> Rectangle {
    let padding = (size * 0.1).round();
    Rectangle {
        x: padding + size * progress,
        y: padding,
        width: size - 2.0 * padding,
        height: size - 2.0 * padding,
    }
}

fn mix(from: iced::Color, to: iced::Color, progress: f32) -> iced::Color {
    iced::Color {
        r: from.r + (to.r - from.r) * progress,
        g: from.g + (to.g - from.g) * progress,
        b: from.b + (to.b - from.b) * progress,
        a: from.a + (to.a - from.a) * progress,
    }
}

impl Tween {
    pub fn new(value: f32, now: Instant) -> Self {
        Self {
            from: value,
            target: value,
            started: now,
            duration: Duration::ZERO,
        }
    }
    pub fn value(&self, now: Instant) -> f32 {
        let progress = if self.duration.is_zero() {
            1.0
        } else {
            (now.saturating_duration_since(self.started).as_secs_f32()
                / self.duration.as_secs_f32())
            .clamp(0.0, 1.0)
        };
        let eased = 1.0 - (1.0 - progress).powi(3);
        self.from + (self.target - self.from) * eased
    }
    pub fn active(&self, now: Instant) -> bool {
        self.from != self.target && now.saturating_duration_since(self.started) < self.duration
    }
    pub fn target(&self) -> f32 {
        self.target
    }
    pub fn to(&mut self, target: f32, duration: Duration, now: Instant) {
        if target != self.target {
            self.from = self.value(now);
            self.target = target;
            self.started = now;
            self.duration = duration.mul_f32((self.target - self.from).abs());
        }
    }
}

pub fn fade<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    opacity: f32,
    interactive: bool,
) -> Element<'a, Message> {
    Element::new(Motion {
        content: content.into(),
        opacity,
        scale: 1.0,
        offset: 0.0,
        interactive,
    })
}

pub fn pop<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    progress: f32,
    interactive: bool,
) -> Element<'a, Message> {
    Element::new(Motion {
        content: content.into(),
        opacity: progress,
        scale: 0.94 + 0.06 * progress,
        offset: 12.0 * (1.0 - progress),
        interactive,
    })
}

struct Motion<'a, Message> {
    content: Element<'a, Message>,
    opacity: f32,
    scale: f32,
    offset: f32,
    interactive: bool,
}

impl<Message> Motion<'_, Message> {
    fn transformation(&self, layout: Layout<'_>) -> Transformation {
        let center = layout.bounds().center();
        Transformation::translate(center.x, center.y + self.offset)
            * Transformation::scale(self.scale)
            * Transformation::translate(-center.x, -center.y)
    }
}

impl<Message> Widget<Message, Theme, Renderer> for Motion<'_, Message> {
    fn tag(&self) -> iced::advanced::widget::tree::Tag {
        self.content.as_widget().tag()
    }
    fn state(&self) -> iced::advanced::widget::tree::State {
        self.content.as_widget().state()
    }
    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }
    fn diff(&self, tree: &mut Tree) {
        self.content.as_widget().diff(tree);
    }
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }
    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content.as_widget_mut().layout(tree, renderer, limits)
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn iced::advanced::widget::Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(tree, layout, renderer, operation);
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if !self.interactive {
            return;
        }
        let inverse = self.transformation(layout).inverse();
        let event = match event {
            Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                Event::Mouse(iced::mouse::Event::CursorMoved {
                    position: *position * inverse,
                })
            }
            event => event.clone(),
        };
        self.content.as_widget_mut().update(
            tree,
            &event,
            layout,
            cursor * inverse,
            renderer,
            clipboard,
            shell,
            &(*viewport * inverse),
        );
    }
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        if self.opacity <= 0.0 {
            return;
        }
        if self.opacity >= 1.0 && self.scale == 1.0 && self.offset == 0.0 {
            self.content
                .as_widget()
                .draw(tree, renderer, theme, style, layout, cursor, viewport);
            return;
        }
        use iced::advanced::Renderer as _;
        let transform = self.transformation(layout);
        let inverse = transform.inverse();
        let previous = renderer.opacity;
        renderer.opacity *= self.opacity.clamp(0.0, 1.0);
        renderer.with_layer(*viewport, |renderer| {
            renderer.with_transformation(transform, |renderer| {
                self.content.as_widget().draw(
                    tree,
                    renderer,
                    theme,
                    style,
                    layout,
                    cursor * inverse,
                    &(*viewport * inverse),
                );
            });
        });
        renderer.opacity = previous;
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        if !self.interactive {
            return mouse::Interaction::None;
        }
        let inverse = self.transformation(layout).inverse();
        self.content.as_widget().mouse_interaction(
            tree,
            layout,
            cursor * inverse,
            &(*viewport * inverse),
            renderer,
        )
    }
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        if !self.interactive {
            return None;
        }
        self.content
            .as_widget_mut()
            .overlay(tree, layout, renderer, viewport, translation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cards_finish_in_50ms_and_switch_reversal_preserves_current_opacity() {
        let start = Instant::now();
        let mut card = Tween::new(0.0, start);
        card.to(1.0, CARD_DURATION, start);
        assert!(card.active(start + Duration::from_millis(49)));
        assert_eq!(card.value(start + Duration::from_millis(50)), 1.0);
        assert!(!card.active(start + Duration::from_millis(50)));

        let mut toggle = Toggle::new(false);
        toggle.set(true, start);
        let middle = start + Duration::from_millis(40);
        let opacity = toggle.motion.value(middle);
        assert!(opacity > 0.0 && opacity < 1.0);
        let position = switch_thumb(26.0, opacity).x;
        assert!(position > switch_thumb(26.0, 0.0).x);
        assert!(position < switch_thumb(26.0, 1.0).x);
        toggle.set(false, middle);
        assert_eq!(toggle.motion.value(middle), opacity);
        assert_eq!(switch_thumb(26.0, toggle.motion.value(middle)).x, position);
        assert!(!toggle.enabled);
        assert!(
            switch_thumb(
                26.0,
                toggle.motion.value(middle + Duration::from_millis(10))
            )
            .x < position
        );
        assert_eq!(toggle.motion.value(middle + SWITCH_DURATION), 0.0);
        assert!(!toggle.active(middle + SWITCH_DURATION));
    }

    #[test]
    fn reversal_is_continuous_and_repeated_dismiss_does_not_restart() {
        let start = Instant::now();
        let mut tween = Tween::new(0.0, start);
        tween.to(1.0, OPEN_DURATION, start);
        let middle = start + Duration::from_millis(70);
        let value = tween.value(middle);
        assert!(value > 0.0 && value < 1.0);
        tween.to(0.0, CLOSE_DURATION, middle);
        assert_eq!(tween.value(middle), value);
        tween.to(0.0, CLOSE_DURATION, middle + Duration::from_millis(20));
        assert!(!tween.active(middle + CLOSE_DURATION));
        assert_eq!(tween.value(middle + CLOSE_DURATION), 0.0);
        tween.to(1.0, OPEN_DURATION, middle + CLOSE_DURATION);
        assert_eq!(tween.value(middle + CLOSE_DURATION + OPEN_DURATION), 1.0);
    }
}
