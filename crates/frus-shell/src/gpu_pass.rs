// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`GpuPass`]: a box an application draws on the GPU itself (milestone 792) — 3D, a
//! compute result, anything wgpu draws — composited with the widgets around it.

use std::marker::PhantomData;

use frus_gpu::{GpuCallback, GpuPassContext};
use frus_widgets::{Dimension, FillAxes, LayoutStyle, Rect, Scene, Status, Theme, Widget};

/// **A box drawn by the application on frus's own GPU device.**
///
/// The function is given the device, the queue, an encoder and a picture the size of the
/// box in physical pixels, cleared to transparent ([`GpuPassContext`]). What it draws is
/// composited like an image: clipped, faded, rounded and transformed with the widgets
/// around it. It runs every frame the box is drawn; with
/// [`playing`](GpuPass::playing) the frames keep coming and `ctx.time` runs, and without
/// it nothing is drawn while the screen is still.
///
/// The wgpu it draws with is frus's: `frus::wgpu`, so an application never pins a version
/// of its own. Keep pipelines and buffers in [`GpuPassContext::state`], made once.
///
/// ```no_run
/// use frus_shell::wgpu;
/// use frus_shell::GpuPass;
///
/// let clear: GpuPass = GpuPass::new(|ctx| {
///     let _pass = ctx.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
///         label: Some("clear"),
///         color_attachments: &[Some(wgpu::RenderPassColorAttachment {
///             view: ctx.target,
///             depth_slice: None,
///             resolve_target: None,
///             ops: wgpu::Operations {
///                 // Premultiplied, linear: a half-transparent green.
///                 load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.0, g: 0.5, b: 0.0, a: 0.5 }),
///                 store: wgpu::StoreOp::Store,
///             },
///         })],
///         depth_stencil_attachment: None,
///         timestamp_writes: None,
///         occlusion_query_set: None,
///         multiview_mask: None,
///     });
/// })
/// .size(200.0, 120.0);
/// ```
pub struct GpuPass<Msg = frus_widgets::Callback> {
    callback: GpuCallback,
    width: Option<f32>,
    height: Option<f32>,
    playing: bool,
    _msg: PhantomData<fn() -> Msg>,
}

impl<Msg> GpuPass<Msg> {
    /// A box drawn by `draw`, every frame it is shown.
    pub fn new<F>(draw: F) -> Self
    where
        F: Fn(&mut GpuPassContext<'_>) + Send + Sync + 'static,
    {
        Self::from_callback(GpuCallback::new(draw))
    }

    /// A box drawn by a callback made beforehand.
    pub fn from_callback(callback: GpuCallback) -> Self {
        Self {
            callback,
            width: None,
            height: None,
            playing: false,
            _msg: PhantomData,
        }
    }

    /// Tells this pass apart from others drawn by the same function, as the rows of a list:
    /// each keeps its own picture and state.
    pub fn key(mut self, key: u64) -> Self {
        self.callback = self.callback.key(key);
        self
    }

    /// Its size in logical pixels. Unset, it fills the room it is given.
    pub fn size(mut self, width: f32, height: f32) -> Self {
        self.width = Some(width);
        self.height = Some(height);
        self
    }

    /// Keeps the frames coming, one per display refresh, with `ctx.time` running: for an
    /// animation. Off, the pass is drawn only when something else redraws the screen.
    pub fn playing(mut self, playing: bool) -> Self {
        self.playing = playing;
        self
    }
}

impl<Msg> Widget<Msg> for GpuPass<Msg> {
    fn style(&self) -> LayoutStyle {
        LayoutStyle {
            width: self.width.map_or(Dimension::Auto, Dimension::Length),
            height: self.height.map_or(Dimension::Auto, Dimension::Length),
            ..Default::default()
        }
    }

    fn fill_axes(&self, _theme: &Theme) -> FillAxes {
        FillAxes {
            horizontal: self.width.is_none(),
            vertical: self.height.is_none(),
        }
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &[]
    }

    fn continuous(&self) -> bool {
        self.playing
    }

    fn paint(&self, bounds: Rect, status: Status, _theme: &Theme, scene: &mut Scene) {
        if status.opacity < 1.0 {
            scene.layer(status.opacity, |s| {
                self.callback.paint(s, bounds, status.time)
            });
        } else {
            self.callback.paint(scene, bounds, status.time);
        }
    }

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn debug_name(&self) -> &'static str {
        "GpuPass"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use frus_widgets::Primitive;

    /// The pass paints its box as a layer carrying the callback, at the frame's time, and
    /// asks for frames only while it plays.
    #[test]
    fn a_pass_paints_its_box() {
        let pass: GpuPass<()> = GpuPass::new(|_ctx| {}).size(40.0, 20.0).playing(true);
        assert!(Widget::<()>::continuous(&pass));
        let status = Status {
            time: 2.0,
            ..Status::default()
        };
        let mut scene = Scene::new();
        pass.paint(
            Rect::new(5.0, 6.0, 40.0, 20.0),
            status,
            &Theme::default(),
            &mut scene,
        );
        let Primitive::Layer { filter, .. } = &scene.primitives()[0] else {
            panic!("a layer");
        };
        let external = filter.external.as_ref().expect("a GPU pass");
        assert_eq!(external.rect(), Rect::new(5.0, 6.0, 40.0, 20.0));
        assert_eq!(external.seconds(), 2.0);
        assert!(external.payload().downcast_ref::<GpuCallback>().is_some());
        let still: GpuPass<()> = GpuPass::new(|_ctx| {});
        assert!(!Widget::<()>::continuous(&still));
    }
}
