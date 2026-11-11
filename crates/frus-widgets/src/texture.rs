// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`Texture`]: shows the latest frame of an **external texture** (milestone 793) — a
//! video, a camera preview, a map, a native view — that a producer feeds from any thread
//! through the [texture registry](frus_core::textures).
//!
//! The widget takes the room it is offered, as a video surface does, and draws the frame
//! inside it by its [`BoxFit`] (stretched to fill unless told otherwise). A new frame
//! repaints it and nothing else: the producer's push asks the shell for one frame, the
//! repaint boundary around the texture is painted again because its
//! [`paint_source`](crate::Widget::paint_source) moved, and the GPU writes the frame into
//! the texture it already has.
//!
//! ```no_run
//! use frus_widgets::Texture;
//!
//! let texture = frus_core::textures().create();
//! let id = texture.id();
//! std::thread::spawn(move || loop {
//!     let frame = vec![0u8; 320 * 240 * 4]; // from a decoder, a camera...
//!     texture.push_rgba(320, 240, &frame).unwrap();
//!     std::thread::sleep(std::time::Duration::from_millis(16));
//! });
//! let view: Texture = Texture::new(id).fit(frus_core::BoxFit::Contain);
//! # let _ = view;
//! ```

use frus_core::{
    Alignment, AlignmentGeometry, BoxFit, Color, FilterQuality, ImageData, Rect, Scene, Size,
    TextureId,
};
use frus_layout::{Dimension, Style};

use crate::interaction::Status;
use crate::theme::Theme;
use crate::widget::Widget;

/// Shows the latest frame of an **external texture**: frames a producer pushes from any thread
/// into the [texture registry](frus_core::textures). It takes the room it is offered and draws
/// the frame by its [`BoxFit`]; a new frame repaints it and nothing else.
pub struct Texture {
    id: TextureId,
    width: Option<f32>,
    height: Option<f32>,
    fit: BoxFit,
    alignment: AlignmentGeometry,
    filter_quality: FilterQuality,
    opacity: f32,
    background: Option<Color>,
    semantic_label: Option<String>,
}

impl Texture {
    /// Shows texture `id`: the room on offer, the frame stretched across it, filtered at
    /// [`FilterQuality::Low`].
    ///
    /// ```
    /// let texture = frus_core::textures().create();
    /// let _view = frus_widgets::Texture::new(texture.id());
    /// ```
    pub fn new(id: TextureId) -> Self {
        Self {
            id,
            width: None,
            height: None,
            fit: BoxFit::Fill,
            alignment: AlignmentGeometry::Physical(Alignment::CENTER),
            filter_quality: FilterQuality::Low,
            opacity: 1.0,
            background: None,
            semantic_label: None,
        }
    }

    /// The texture shown.
    pub fn id(&self) -> TextureId {
        self.id
    }

    /// A fixed width, in logical pixels, instead of the room on offer.
    ///
    /// ```
    /// # let texture = frus_core::textures().create();
    /// let _view = frus_widgets::Texture::new(texture.id()).width(320.0).height(180.0);
    /// ```
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// A fixed height, in logical pixels, instead of the room on offer.
    pub fn height(mut self, height: f32) -> Self {
        self.height = Some(height);
        self
    }

    /// How the frame fits the box: [`BoxFit::Fill`] by default; [`BoxFit::Contain`] keeps a
    /// video's proportions, [`BoxFit::Cover`] crops to fill.
    ///
    /// ```
    /// # let texture = frus_core::textures().create();
    /// let _view = frus_widgets::Texture::new(texture.id()).fit(frus_core::BoxFit::Contain);
    /// ```
    pub fn fit(mut self, fit: BoxFit) -> Self {
        self.fit = fit;
        self
    }

    /// Where a frame that does not fill the box sits in it: the centre by default.
    pub fn alignment(mut self, alignment: impl Into<AlignmentGeometry>) -> Self {
        self.alignment = alignment.into();
        self
    }

    /// How the frame's pixels are filtered when drawn at another size:
    /// [`FilterQuality::Low`] (bilinear) by default, [`FilterQuality::None`] for crisp
    /// pixels. A frame changes every time, so no mip chain is made whatever is asked.
    ///
    /// ```
    /// # let texture = frus_core::textures().create();
    /// let _view = frus_widgets::Texture::new(texture.id())
    ///     .filter_quality(frus_core::FilterQuality::None);
    /// ```
    pub fn filter_quality(mut self, quality: FilterQuality) -> Self {
        self.filter_quality = quality;
        self
    }

    /// The frame's opacity, `0..1`.
    pub fn opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity.clamp(0.0, 1.0);
        self
    }

    /// A colour painted behind the frame: the bars around a contained video, and what
    /// shows before the first frame. Transparent by default.
    ///
    /// ```
    /// # let texture = frus_core::textures().create();
    /// let _view = frus_widgets::Texture::new(texture.id()).background(frus_core::Color::BLACK);
    /// ```
    pub fn background(mut self, color: Color) -> Self {
        self.background = Some(color);
        self
    }

    /// What a screen reader says the texture shows ("Product video").
    pub fn semantic_label(mut self, label: impl Into<String>) -> Self {
        self.semantic_label = Some(label.into());
        self
    }

    /// Where the frame is drawn in `bounds`, and which part of it: `None` before the first
    /// frame or once the texture is gone.
    fn placement(&self, bounds: Rect) -> Option<(ImageData, Rect, Rect)> {
        let info = frus_core::textures().frame_info(self.id)?;
        let size = Size::new(info.width as f32, info.height as f32);
        let align = self.alignment.resolve(frus_core::TextDirection::Ltr);
        let (dst, uv) = self.fit.apply_aligned(size, bounds, align);
        let image = ImageData::external(self.id, info.generation, info.width, info.height);
        Some((image, dst, uv))
    }
}

impl<Msg> Widget<Msg> for Texture {
    fn style(&self) -> Style {
        // The room on offer, as the reference's texture takes it: a surface has no size of
        // its own worth asking for, and its frames' size may change at any time.
        let side = |given: Option<f32>| given.map_or(Dimension::Percent(1.0), Dimension::Length);
        Style {
            width: side(self.width),
            height: side(self.height),
            min_width: Dimension::Length(0.0),
            min_height: Dimension::Length(0.0),
            ..Default::default()
        }
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &[]
    }

    fn paint(&self, bounds: Rect, status: Status, _theme: &Theme, scene: &mut Scene) {
        let opacity = status.opacity * self.opacity;
        if let Some(color) = self.background {
            scene.fill_rect(bounds, color.fade(opacity));
        }
        let Some((image, dst, uv)) = self.placement(bounds) else {
            return;
        };
        let tint = Color::WHITE.fade(opacity);
        scene.draw_image_with(
            &image.into_handle(),
            dst,
            uv,
            tint,
            false,
            self.filter_quality,
        );
    }

    fn paint_source(&self) -> Option<u64> {
        // The frame on show, or nought before the first: either way a change repaints.
        Some(
            frus_core::textures()
                .frame_info(self.id)
                .map_or(0, |info| info.generation),
        )
    }

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn semantics(&self) -> Option<frus_core::SemanticsProperties> {
        let mut semantics = frus_core::SemanticsProperties::new(frus_core::Role::Image);
        if let Some(label) = self.semantic_label.as_deref() {
            semantics = semantics.label(label);
        }
        Some(semantics)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use frus_core::Primitive;

    fn paint(texture: &Texture, bounds: Rect) -> Vec<Primitive> {
        let mut scene = Scene::new();
        Widget::<()>::paint(
            texture,
            bounds,
            Status::default(),
            &Theme::default(),
            &mut scene,
        );
        scene.primitives().to_vec()
    }

    #[test]
    fn nothing_is_drawn_before_the_first_frame_or_after_the_texture_is_gone() {
        let texture = frus_core::textures().create();
        let view = Texture::new(texture.id());
        let bounds = Rect::new(0.0, 0.0, 100.0, 50.0);
        assert!(paint(&view, bounds).is_empty());
        texture.push_rgba(2, 1, &[0; 8]).unwrap();
        assert_eq!(paint(&view, bounds).len(), 1);
        drop(texture);
        assert!(paint(&view, bounds).is_empty());
    }

    #[test]
    fn the_frame_is_fitted_and_names_its_generation() {
        let texture = frus_core::textures().create();
        texture.push_rgba(4, 2, &[0; 32]).unwrap();
        let view = Texture::new(texture.id()).fit(BoxFit::Contain);
        let prims = paint(&view, Rect::new(0.0, 0.0, 100.0, 100.0));
        let Primitive::Image { image, rect, .. } = &prims[0] else {
            panic!("an image: {prims:?}");
        };
        // 2:1 contained in a square: full width, half height, centred.
        assert_eq!(*rect, Rect::new(0.0, 25.0, 100.0, 50.0));
        assert_eq!(image.external_frame(), Some((texture.id(), 1)));
        texture.push_rgba(4, 2, &[0; 32]).unwrap();
        let prims = paint(&view, Rect::new(0.0, 0.0, 100.0, 100.0));
        let Primitive::Image { image, .. } = &prims[0] else {
            panic!();
        };
        assert_eq!(image.external_frame(), Some((texture.id(), 2)));
    }

    #[test]
    fn a_new_frame_moves_the_paint_source() {
        let texture = frus_core::textures().create();
        let view = Texture::new(texture.id());
        let before = Widget::<()>::paint_source(&view);
        texture.push_rgba(1, 1, &[0; 4]).unwrap();
        let after = Widget::<()>::paint_source(&view);
        assert_ne!(before, after);
        assert_eq!(
            after,
            Widget::<()>::paint_source(&view),
            "steady without a push"
        );
    }

    #[test]
    fn the_background_shows_behind_and_before_the_frame() {
        let texture = frus_core::textures().create();
        let view = Texture::new(texture.id()).background(Color::BLACK);
        let prims = paint(&view, Rect::new(0.0, 0.0, 10.0, 10.0));
        assert!(matches!(prims.as_slice(), [Primitive::Rect { .. }]));
    }

    #[test]
    fn it_takes_the_room_unless_given_a_size() {
        let texture = frus_core::textures().create();
        let style = Widget::<()>::style(&Texture::new(texture.id()));
        assert_eq!(style.width, Dimension::Percent(1.0));
        let style = Widget::<()>::style(&Texture::new(texture.id()).width(320.0));
        assert_eq!(style.width, Dimension::Length(320.0));
    }
}
