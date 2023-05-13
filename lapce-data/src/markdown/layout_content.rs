use std::sync::Arc;

use druid::{
    kurbo::Line,
    piet::{InterpolationMode, PietText},
    ArcStr, Color, Env, EventCtx, ExtEventSink, FontDescriptor, PaintCtx, Point,
    Rect, RenderContext, Size, TextLayout, UpdateCtx, Vec2,
};
use lsp_types::Url;

use super::Content;
use crate::{
    config::{LapceConfig, LapceIcons, LapceTheme},
    data::LapceTabData,
    images::{Image, ImageCache, ImageStatus},
    rich_text::RichText,
};

#[derive(Clone)]
pub enum LayoutContent {
    Text(TextLayout<RichText>),
    // TODO: keep title with it to inform the user on hover
    Image { url: Url, size: Size },
    BrokenImage { text: TextLayout<RichText> },
    Separator { width: f64 },
}
impl LayoutContent {
    // TODO: Add configuration for whether it should allow loading local file urls
    // (And it would be desirable to have them be fine-grained enough to allow previewing
    // local markdown files to load local images, but not allow loading local images in hover)

    /// Transform a `MarkdownContent` instance into one which can be rendered.  
    /// This loads images as needed. Specify a `base_url` for the root of relative urls, typically
    /// the current workspace.  
    pub fn from_content(
        event_sink: ExtEventSink,
        images: &mut ImageCache,
        base_url: Option<&Url>,
        content: &Content,
    ) -> LayoutContent {
        match content {
            Content::Text(text) => {
                let mut layout = TextLayout::new();
                layout.set_text(text.clone());
                LayoutContent::Text(layout)
            }
            Content::Image { url, .. } => {
                if let Ok(url) = Url::options().base_url(base_url).parse(url) {
                    images.load_url_cmd(url.clone(), event_sink);

                    LayoutContent::Image {
                        url,
                        size: Size::ZERO,
                    }
                } else {
                    // TODO: highlight this with red text and make it italics or something?
                    let mut layout = TextLayout::new();
                    layout.set_text(RichText::new(ArcStr::from(format!(
                        "Bad Image URL: {url}"
                    ))));
                    LayoutContent::BrokenImage { text: layout }
                }
            }
            Content::Separator => LayoutContent::Separator { width: 0.0 },
        }
    }

    pub fn set_font(&mut self, font: FontDescriptor) {
        match self {
            LayoutContent::Text(layout) => {
                layout.set_font(font);
            }
            LayoutContent::BrokenImage { text } => {
                text.set_font(font);
            }
            LayoutContent::Image { .. } | LayoutContent::Separator { .. } => {}
        }
    }

    pub fn set_text_color(&mut self, color: Color) {
        match self {
            LayoutContent::Text(layout) => {
                layout.set_text_color(color);
            }
            LayoutContent::BrokenImage { text } => {
                text.set_text_color(color);
            }
            LayoutContent::Image { .. } | LayoutContent::Separator { .. } => {}
        }
    }

    pub fn set_max_width(&mut self, images: &ImageCache, max_width: f64) {
        match self {
            LayoutContent::Text(layout) => {
                layout.set_wrap_width(max_width);
            }
            LayoutContent::BrokenImage { text } => {
                text.set_wrap_width(max_width);
            }
            LayoutContent::Image { url, size } => {
                if let Some(ImageStatus::Loaded(image)) = images.get(url) {
                    *size = get_image_size(image, max_width);
                }
            }
            LayoutContent::Separator { width } => {
                *width = max_width - 2.0;
            }
        }
    }

    pub fn needs_rebuild_after_update(&mut self, ctx: &mut UpdateCtx) -> bool {
        match self {
            LayoutContent::Text(layout) => layout.needs_rebuild_after_update(ctx),
            LayoutContent::BrokenImage { text } => {
                text.needs_rebuild_after_update(ctx)
            }
            LayoutContent::Image { .. } | LayoutContent::Separator { .. } => false,
        }
    }

    pub fn rebuild_if_needed(&mut self, factory: &mut PietText, env: &Env) {
        match self {
            LayoutContent::Text(layout) => {
                layout.rebuild_if_needed(factory, env);
            }
            LayoutContent::BrokenImage { text } => {
                text.rebuild_if_needed(factory, env);
            }
            LayoutContent::Image { .. } | LayoutContent::Separator { .. } => {}
        }
    }

    /// `width` is only used for images currently, and should probably be the same as
    /// `set_wrap_width`'s call
    pub fn size(&self, images: &ImageCache, config: &LapceConfig) -> Size {
        match self {
            LayoutContent::Text(layout) => layout.size(),
            LayoutContent::Image { url, size } => match images.get(url) {
                Some(ImageStatus::Loading | ImageStatus::Error) | None => {
                    get_svg_size(config)
                }
                Some(ImageStatus::Loaded(_)) => *size,
            },
            LayoutContent::BrokenImage { text } => text.size(),
            // TODO: Separator side margin should perhaps be supplied by some margin setting??
            LayoutContent::Separator { width } => Size::new(*width, 1.0),
        }
    }

    /// `width` is only used for images currently, and should probably be the same as
    /// `set_wrap_width`'s call
    pub fn draw(
        &self,
        ctx: &mut PaintCtx,
        images: &ImageCache,
        config: &LapceConfig,
        origin: Point,
    ) {
        match self {
            LayoutContent::Text(layout) => {
                layout.draw(ctx, origin);
            }
            LayoutContent::Image { url, size } => {
                match images.get(url) {
                    Some(ImageStatus::Loading) => {
                        // TODO: Animating this as spinning would be nice, but would be easier
                        // to do once we have some general textview widget to hold renderalbe
                        // content
                        ctx.draw_svg(
                            &config.ui_svg(LapceIcons::IMAGE_LOADING),
                            get_svg_size(config).to_rect().with_origin(origin),
                            Some(
                                config.get_color_unchecked(
                                    LapceTheme::LAPCE_ICON_ACTIVE,
                                ),
                            ),
                        );
                    }
                    Some(ImageStatus::Error) | None => {
                        // TODO: On hover, give the user the information about image that
                        // failed to load
                        ctx.draw_svg(
                            &config.ui_svg(LapceIcons::IMAGE_ERROR),
                            get_svg_size(config).to_rect().with_origin(origin),
                            Some(
                                config.get_color_unchecked(
                                    LapceTheme::LAPCE_ICON_ACTIVE,
                                ),
                            ),
                        );
                    }
                    Some(ImageStatus::Loaded(image)) => match image {
                        Image::Image(image) => {
                            ctx.draw_image(
                                image,
                                Rect::from_origin_size(origin, *size),
                                InterpolationMode::Bilinear,
                            );
                        }
                    },
                }
            }
            LayoutContent::BrokenImage { text } => {
                text.draw(ctx, origin);
            }
            LayoutContent::Separator { width } => {
                let line = Line::new(
                    origin + Vec2::new(1.0, 0.0),
                    origin + Vec2::new(1.0 + *width, 0.0),
                );
                ctx.stroke(
                    line,
                    config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                    1.0,
                );
            }
        }
    }
}

fn get_svg_size(config: &LapceConfig) -> Size {
    let svg_size = config.ui.icon_size() as f64;
    Size::new(svg_size, svg_size)
}

fn get_image_size(image: &Image, max_width: f64) -> Size {
    match image {
        Image::Image(image) => {
            // This needs piet-wgpu updated with the size implementation for WgpuImage
            // let size = image.size();
            let (width, height) = image.img.dimensions();
            let size = Size::new(width as f64, height as f64);
            let aspect_ratio = size.width / size.height;
            let height = max_width / aspect_ratio;
            Size::new(max_width, height)
        }
    }
}

/// Utility function to make constructing a vector of `LayoutContent` from a vector of `Content`
pub fn layouts_from_contents<'a>(
    ctx: &mut EventCtx,
    data: &mut LapceTabData,
    items: impl Iterator<Item = &'a Content>,
) -> Vec<LayoutContent> {
    let event_sink = ctx.get_external_handle();
    let images = Arc::make_mut(&mut data.images);
    let base_url = data
        .workspace
        .path
        .as_deref()
        .and_then(|p| Url::from_directory_path(p).ok());
    let base_url = base_url.as_ref();
    let mut layouts = Vec::new();

    for content in items {
        layouts.push(LayoutContent::from_content(
            event_sink.clone(),
            images,
            base_url,
            content,
        ));
    }

    layouts
}

/// Mark that you're done with the layout content, so that they can be cleaned up
pub fn layout_content_clean_up(
    layouts: &mut Vec<LayoutContent>,
    data: &mut LapceTabData,
) {
    let images = Arc::make_mut(&mut data.images);
    for item in layouts.drain(..) {
        if let LayoutContent::Image { url, .. } = item {
            images.done_with_image(&url);
        }
    }
}
