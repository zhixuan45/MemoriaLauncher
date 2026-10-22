//! Alpha modulation shared by widgets, including images and SVGs, on both backends.
use iced::advanced::{graphics, image, renderer, svg, text};
use iced::{Background, Color, Font, Pixels, Point, Rectangle, Size, Transformation};

pub type Element<'a, Message> = iced::Element<'a, Message, iced::Theme, Renderer>;

pub struct Renderer {
    inner: iced::Renderer,
    pub opacity: f32,
}

impl Renderer {
    fn new(inner: iced::Renderer) -> Self {
        Self {
            inner,
            opacity: 1.0,
        }
    }
}

impl renderer::Renderer for Renderer {
    fn start_layer(&mut self, bounds: Rectangle) {
        self.inner.start_layer(bounds);
    }
    fn end_layer(&mut self) {
        self.inner.end_layer();
    }
    fn start_transformation(&mut self, transform: Transformation) {
        self.inner.start_transformation(transform);
    }
    fn end_transformation(&mut self) {
        self.inner.end_transformation();
    }
    fn fill_quad(&mut self, mut quad: renderer::Quad, background: impl Into<Background>) {
        quad.border.color = quad.border.color.scale_alpha(self.opacity);
        quad.shadow.color = quad.shadow.color.scale_alpha(self.opacity);
        self.inner
            .fill_quad(quad, background.into().scale_alpha(self.opacity));
    }
    fn reset(&mut self, bounds: Rectangle) {
        self.opacity = 1.0;
        self.inner.reset(bounds);
    }
    fn allocate_image(
        &mut self,
        handle: &image::Handle,
        callback: impl FnOnce(Result<image::Allocation, image::Error>) + Send + 'static,
    ) {
        self.inner.allocate_image(handle, callback);
    }
}

impl text::Renderer for Renderer {
    type Font = Font;
    type Paragraph = <iced::Renderer as text::Renderer>::Paragraph;
    type Editor = <iced::Renderer as text::Renderer>::Editor;
    const ICON_FONT: Font = <iced::Renderer as text::Renderer>::ICON_FONT;
    const CHECKMARK_ICON: char = <iced::Renderer as text::Renderer>::CHECKMARK_ICON;
    const ARROW_DOWN_ICON: char = <iced::Renderer as text::Renderer>::ARROW_DOWN_ICON;
    const SCROLL_UP_ICON: char = <iced::Renderer as text::Renderer>::SCROLL_UP_ICON;
    const SCROLL_DOWN_ICON: char = <iced::Renderer as text::Renderer>::SCROLL_DOWN_ICON;
    const SCROLL_LEFT_ICON: char = <iced::Renderer as text::Renderer>::SCROLL_LEFT_ICON;
    const SCROLL_RIGHT_ICON: char = <iced::Renderer as text::Renderer>::SCROLL_RIGHT_ICON;
    const ICED_LOGO: char = <iced::Renderer as text::Renderer>::ICED_LOGO;
    fn default_font(&self) -> Font {
        self.inner.default_font()
    }
    fn default_size(&self) -> Pixels {
        self.inner.default_size()
    }
    fn fill_paragraph(
        &mut self,
        paragraph: &Self::Paragraph,
        position: Point,
        color: Color,
        clip: Rectangle,
    ) {
        self.inner
            .fill_paragraph(paragraph, position, color.scale_alpha(self.opacity), clip);
    }
    fn fill_editor(
        &mut self,
        editor: &Self::Editor,
        position: Point,
        color: Color,
        clip: Rectangle,
    ) {
        self.inner
            .fill_editor(editor, position, color.scale_alpha(self.opacity), clip);
    }
    fn fill_text(
        &mut self,
        text: text::Text<String, Font>,
        position: Point,
        color: Color,
        clip: Rectangle,
    ) {
        self.inner
            .fill_text(text, position, color.scale_alpha(self.opacity), clip);
    }
}

impl image::Renderer for Renderer {
    type Handle = image::Handle;
    fn load_image(&self, handle: &Self::Handle) -> Result<image::Allocation, image::Error> {
        self.inner.load_image(handle)
    }
    fn measure_image(&self, handle: &Self::Handle) -> Option<Size<u32>> {
        self.inner.measure_image(handle)
    }
    fn draw_image(
        &mut self,
        mut image: iced::advanced::graphics::core::Image,
        bounds: Rectangle,
        clip: Rectangle,
    ) {
        image.opacity *= self.opacity;
        self.inner.draw_image(image, bounds, clip);
    }
}

impl svg::Renderer for Renderer {
    fn measure_svg(&self, handle: &svg::Handle) -> Size<u32> {
        self.inner.measure_svg(handle)
    }
    fn draw_svg(
        &mut self,
        mut svg: iced::advanced::graphics::core::Svg,
        bounds: Rectangle,
        clip: Rectangle,
    ) {
        svg.opacity *= self.opacity;
        self.inner.draw_svg(svg, bounds, clip);
    }
}

impl renderer::Headless for Renderer {
    async fn new(font: Font, size: Pixels, backend: Option<&str>) -> Option<Self> {
        <iced::Renderer as renderer::Headless>::new(font, size, backend)
            .await
            .map(Self::new)
    }
    fn name(&self) -> String {
        self.inner.name()
    }
    fn screenshot(&mut self, size: Size<u32>, scale: f32, background: Color) -> Vec<u8> {
        self.inner.screenshot(size, scale, background)
    }
}

type NativeCompositor = <iced::Renderer as graphics::compositor::Default>::Compositor;
pub struct Compositor(NativeCompositor);

impl graphics::compositor::Default for Renderer {
    type Compositor = Compositor;
}

impl graphics::Compositor for Compositor {
    type Renderer = Renderer;
    type Surface = <NativeCompositor as graphics::Compositor>::Surface;
    async fn with_backend(
        settings: graphics::Settings,
        display: impl graphics::compositor::Display + Clone,
        window: impl graphics::compositor::Window + Clone,
        shell: graphics::Shell,
        backend: Option<&str>,
    ) -> Result<Self, graphics::Error> {
        NativeCompositor::with_backend(settings, display, window, shell, backend)
            .await
            .map(Self)
    }
    fn create_renderer(&self) -> Renderer {
        Renderer::new(self.0.create_renderer())
    }
    fn create_surface<W: graphics::compositor::Window + Clone>(
        &mut self,
        window: W,
        width: u32,
        height: u32,
    ) -> Self::Surface {
        self.0.create_surface(window, width, height)
    }
    fn configure_surface(&mut self, surface: &mut Self::Surface, width: u32, height: u32) {
        self.0.configure_surface(surface, width, height);
    }
    fn information(&self) -> graphics::compositor::Information {
        self.0.information()
    }
    fn load_font(&mut self, font: std::borrow::Cow<'static, [u8]>) {
        self.0.load_font(font);
    }
    fn present(
        &mut self,
        renderer: &mut Renderer,
        surface: &mut Self::Surface,
        viewport: &graphics::Viewport,
        background: Color,
        on_pre_present: impl FnOnce(),
    ) -> Result<(), graphics::compositor::SurfaceError> {
        self.0.present(
            &mut renderer.inner,
            surface,
            viewport,
            background,
            on_pre_present,
        )
    }
    fn screenshot(
        &mut self,
        renderer: &mut Renderer,
        viewport: &graphics::Viewport,
        background: Color,
    ) -> Vec<u8> {
        self.0.screenshot(&mut renderer.inner, viewport, background)
    }
}
