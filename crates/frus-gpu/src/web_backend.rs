// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Which GPU interface a page draws through** (milestone 745): WebGPU where the browser
//! offers it, WebGL 2 where it does not.
//!
//! A browser can lack WebGPU in three ways: no `navigator.gpu` at all (Chrome on Android
//! before Android 12, older Safari and Firefox), an API with no adapter behind it (a GPU or
//! driver the browser has blocked), or an adapter that cannot make a device. The choice is
//! made **before the canvas is given a context**: a canvas that has handed out a `webgpu`
//! context never hands out a `webgl2` one, so the WebGPU path is only taken once a device
//! has been made, and the surface comes last.
//!
//! A browser can also make a WebGPU device and then fail at the canvas (milestone 847): no
//! `webgpu` context for it, a format it lists and then refuses to configure, no frame to draw
//! in. The device is therefore tried on a **canvas of its own** first, one the page never
//! shows: configured, drawn in once, its errors collected. Only when that works is the page's
//! canvas given a WebGPU context; when it does not, the page's canvas is still untouched and
//! WebGL 2 draws in it.

/// What the browser's WebGPU turned out to offer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WebGpu {
    /// WebGPU was not among the backends asked for: it was not probed.
    NotAsked,
    /// No `navigator.gpu`: the browser has no WebGPU (or not on this system).
    NoApi,
    /// `navigator.gpu` is there, but the browser gives no adapter: typically a GPU or a
    /// driver it has blocked.
    NoAdapter,
    /// An adapter, but no device could be made from it: why.
    NoDevice(String),
    /// A device, but no canvas could be drawn in through it: why.
    NoSurface(String),
    /// An adapter and a device: WebGPU is used.
    Ready,
}

impl std::fmt::Display for WebGpu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WebGpu::NotAsked => f.write_str("WebGPU was not asked for"),
            WebGpu::NoApi => f.write_str("this browser has no WebGPU (no navigator.gpu)"),
            WebGpu::NoAdapter => {
                f.write_str("this browser's WebGPU gives no adapter (a blocked GPU or driver)")
            }
            WebGpu::NoDevice(why) => write!(f, "this browser's WebGPU made no device: {why}"),
            WebGpu::NoSurface(why) => {
                write!(f, "this browser's WebGPU cannot draw in a canvas: {why}")
            }
            WebGpu::Ready => f.write_str("WebGPU is available"),
        }
    }
}

/// The backend a page draws through, from the backends asked for and what WebGPU offered:
/// WebGPU when it is ready, WebGL 2 (`GL`) otherwise, and an error naming both when neither
/// can be had.
pub(crate) fn choose(requested: wgpu::Backends, webgpu: &WebGpu) -> Result<wgpu::Backends, String> {
    if requested.contains(wgpu::Backends::BROWSER_WEBGPU) && *webgpu == WebGpu::Ready {
        return Ok(wgpu::Backends::BROWSER_WEBGPU);
    }
    if requested.contains(wgpu::Backends::GL) {
        return Ok(wgpu::Backends::GL);
    }
    Err(match webgpu {
        WebGpu::NotAsked => format!("no browser GPU interface among the backends {requested:?}"),
        other => format!("{other}, and WebGL 2 was not asked for"),
    })
}

/// The backends a page asks for: WebGPU first, WebGL 2 behind it.
#[cfg(target_arch = "wasm32")]
pub(crate) const REQUESTED: wgpu::Backends =
    wgpu::Backends::BROWSER_WEBGPU.union(wgpu::Backends::GL);

/// A device made through WebGPU before any surface: what the renderer goes on with.
#[cfg(target_arch = "wasm32")]
pub(crate) struct Opened {
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

/// The instance a page draws through, and — on WebGPU — the adapter and device already made.
/// On WebGL 2 the adapter comes from the canvas's context, so the caller makes the surface
/// first and asks for the adapter with it.
#[cfg(target_arch = "wasm32")]
pub(crate) async fn open(
    requested: wgpu::Backends,
) -> anyhow::Result<(wgpu::Instance, Option<Opened>)> {
    let instance = |backends| {
        wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        })
    };
    let mut probe = WebGpu::NotAsked;
    if requested.contains(wgpu::Backends::BROWSER_WEBGPU) {
        probe = if !has_navigator_gpu() {
            WebGpu::NoApi
        } else {
            // Not before `navigator.gpu` is known to be there: without it, the instance would
            // be wgpu's own, which knows no WebGPU.
            let webgpu = instance(wgpu::Backends::BROWSER_WEBGPU);
            match webgpu
                .request_adapter(&wgpu::RequestAdapterOptions::default())
                .await
            {
                Err(_) => WebGpu::NoAdapter,
                Ok(adapter) => match crate::renderer::request_device(&adapter).await {
                    Err(err) => WebGpu::NoDevice(format!("{err:#}")),
                    Ok((device, queue)) => {
                        match try_canvas(&webgpu, &adapter, &device, &queue).await {
                            Err(why) => {
                                device.destroy();
                                WebGpu::NoSurface(why)
                            }
                            Ok(()) => {
                                log::info!("GPU interface: WebGPU");
                                return Ok((
                                    webgpu,
                                    Some(Opened {
                                        adapter,
                                        device,
                                        queue,
                                    }),
                                ));
                            }
                        }
                    }
                },
            }
        };
    }
    let backends = choose(requested, &probe).map_err(anyhow::Error::msg)?;
    log::info!("GPU interface: WebGL 2 ({probe})");
    Ok((instance(backends), None))
}

/// The instance WebGL 2 is drawn through, for a page whose WebGPU fell through after the
/// first choice: the page's canvas refused a `webgpu` context without being given one.
#[cfg(target_arch = "wasm32")]
pub(crate) fn webgl2_instance() -> wgpu::Instance {
    wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::GL,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    })
}

/// The canvas a window draws in, from its handle: what wgpu itself reads to make the surface.
#[cfg(target_arch = "wasm32")]
pub(crate) fn canvas_of(
    window: &(impl wgpu::rwh::HasWindowHandle + ?Sized),
) -> Option<web_sys::HtmlCanvasElement> {
    use wasm_bindgen::JsCast;
    use wgpu::rwh::RawWindowHandle;
    match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::WebCanvas(handle) => {
            // SAFETY: a web canvas handle points at the canvas's `JsValue`, which the window
            // keeps for as long as it lives; the window is borrowed here. wgpu reads the
            // handle the same way to make the surface.
            let value: &wasm_bindgen::JsValue = unsafe { handle.obj.cast().as_ref() };
            value.clone().dyn_into().ok()
        }
        RawWindowHandle::Web(handle) => web_sys::window()?
            .document()?
            .query_selector(&format!("[data-raw-handle=\"{}\"]", handle.id))
            .ok()??
            .dyn_into()
            .ok(),
        _ => None,
    }
}

/// The side of the canvas the device is tried on: as small as a texture gets, never shown.
#[cfg(target_arch = "wasm32")]
const TRIAL_SIDE: u32 = 4;

/// Draws once through `device` in a canvas that is never shown, the way the renderer will in
/// the page's: a `webgpu` context, configured as the renderer configures it, a frame cleared
/// and presented. `Err` says what failed, the page's canvas not having been touched.
///
/// Where there is no document (a worker) there is nothing to try on, and the device is taken
/// as it is.
#[cfg(target_arch = "wasm32")]
async fn try_canvas(
    instance: &wgpu::Instance,
    adapter: &wgpu::Adapter,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> Result<(), String> {
    use wasm_bindgen::JsCast;
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return Ok(());
    };
    let canvas: web_sys::HtmlCanvasElement = document
        .create_element("canvas")
        .map_err(|err| format!("no canvas could be made: {err:?}"))?
        .dyn_into()
        .map_err(|_| "no canvas could be made".to_owned())?;
    canvas.set_width(TRIAL_SIDE);
    canvas.set_height(TRIAL_SIDE);
    let surface = instance
        .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
        .map_err(|err| err.to_string())?;
    let caps = surface.get_capabilities(adapter);
    let (format, render_format, view_formats) =
        crate::renderer::canvas_formats(adapter, &caps).ok_or("the canvas offers no format")?;
    let alpha_mode = caps
        .alpha_modes
        .first()
        .copied()
        .unwrap_or(wgpu::CompositeAlphaMode::Auto);
    // Validation errors would otherwise reach the device's handler, which ends the page.
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    surface.configure(
        device,
        &wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width: TRIAL_SIDE,
            height: TRIAL_SIDE,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode,
            view_formats,
            desired_maximum_frame_latency: 2,
        },
    );
    let drawn = match surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(frame)
        | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
            let view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
                format: Some(render_format),
                ..Default::default()
            });
            let mut encoder = device.create_command_encoder(&Default::default());
            encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("frus.canvas-trial"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            queue.submit([encoder.finish()]);
            queue.present(frame);
            Ok(())
        }
        other => Err(format!("no frame to draw in ({})", frame_failure(&other))),
    };
    let validation = scope.pop().await;
    surface_trial(drawn, validation.map(|err| err.to_string()))
}

/// Why a canvas gave no frame, in words.
#[cfg(target_arch = "wasm32")]
fn frame_failure(outcome: &wgpu::CurrentSurfaceTexture) -> &'static str {
    match outcome {
        wgpu::CurrentSurfaceTexture::Success(_) | wgpu::CurrentSurfaceTexture::Suboptimal(_) => {
            "none"
        }
        wgpu::CurrentSurfaceTexture::Timeout => "timed out",
        wgpu::CurrentSurfaceTexture::Occluded => "occluded",
        wgpu::CurrentSurfaceTexture::Outdated => "outdated",
        wgpu::CurrentSurfaceTexture::Lost => "the context was refused or lost",
        wgpu::CurrentSurfaceTexture::Validation => "a validation error",
    }
}

/// The verdict of a canvas trial: what drawing said, and the validation error the device
/// collected meanwhile, if any. Either one fails it.
pub(crate) fn surface_trial(
    drawn: Result<(), String>,
    validation: Option<String>,
) -> Result<(), String> {
    drawn?;
    match validation {
        Some(err) => Err(format!("the canvas refused its configuration: {err}")),
        None => Ok(()),
    }
}

/// Whether the page's global (a window or a worker) has a `navigator.gpu`.
#[cfg(target_arch = "wasm32")]
fn has_navigator_gpu() -> bool {
    use wasm_bindgen::JsValue;
    let navigator = js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("navigator"))
        .unwrap_or(JsValue::UNDEFINED);
    if navigator.is_undefined() || navigator.is_null() {
        return false;
    }
    js_sys::Reflect::get(&navigator, &JsValue::from_str("gpu"))
        .map(|gpu| !gpu.is_undefined() && !gpu.is_null())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgpu::Backends;

    const BOTH: Backends = Backends::BROWSER_WEBGPU.union(Backends::GL);

    #[test]
    fn webgpu_is_used_when_it_is_ready() {
        assert_eq!(choose(BOTH, &WebGpu::Ready), Ok(Backends::BROWSER_WEBGPU));
    }

    /// No API (an Android phone before Android 12), no adapter (a blocked GPU) or no device:
    /// WebGL 2 every time.
    #[test]
    fn webgl2_is_used_whenever_webgpu_is_missing() {
        for missing in [
            WebGpu::NoApi,
            WebGpu::NoAdapter,
            WebGpu::NoDevice("limits".into()),
            WebGpu::NotAsked,
        ] {
            assert_eq!(choose(BOTH, &missing), Ok(Backends::GL), "{missing}");
        }
        assert_eq!(choose(Backends::GL, &WebGpu::NotAsked), Ok(Backends::GL));
    }

    /// A device that cannot draw in a canvas — no `webgpu` context, a format refused at
    /// configuration, no frame — sends the page to WebGL 2 too (milestone 847).
    #[test]
    fn a_device_that_cannot_draw_in_a_canvas_falls_back_to_webgl2() {
        let refused = WebGpu::NoSurface("canvas.getContext() returned null".into());
        assert_eq!(choose(BOTH, &refused), Ok(Backends::GL));
        assert!(
            refused.to_string().contains("cannot draw in a canvas"),
            "{refused}"
        );
        let err = choose(Backends::BROWSER_WEBGPU, &refused).unwrap_err();
        assert!(err.contains("getContext"), "{err}");
    }

    /// The trial fails on either a frame not drawn or a validation error collected while
    /// drawing it, and passes only when neither happened.
    #[test]
    fn a_canvas_trial_fails_on_either_failure() {
        assert_eq!(surface_trial(Ok(()), None), Ok(()));
        assert_eq!(
            surface_trial(Err("no frame to draw in (lost)".into()), None),
            Err("no frame to draw in (lost)".into())
        );
        let err = surface_trial(Ok(()), Some("format rgba16float".into())).unwrap_err();
        assert!(err.contains("refused its configuration"), "{err}");
        assert!(err.contains("rgba16float"), "{err}");
        // The frame's own failure is the one said, when there are both.
        assert_eq!(
            surface_trial(Err("no frame".into()), Some("late".into())),
            Err("no frame".into())
        );
    }

    /// WebGL 2 alone does not take a WebGPU that is ready: the backends asked for decide.
    #[test]
    fn the_backends_asked_for_decide() {
        assert_eq!(choose(Backends::GL, &WebGpu::Ready), Ok(Backends::GL));
    }

    #[test]
    fn neither_is_an_error_that_says_why() {
        let err = choose(Backends::BROWSER_WEBGPU, &WebGpu::NoAdapter).unwrap_err();
        assert!(err.contains("no adapter"), "{err}");
        assert!(err.contains("WebGL 2"), "{err}");
        assert!(choose(Backends::VULKAN, &WebGpu::NotAsked).is_err());
    }
}

/// What WebGL 2 asks of every shader frus draws with (milestone 745), checked on the shaders'
/// own source, since no test host here has WebGL: no storage buffers, no compute stage, and
/// uniforms whose size is a multiple of 16 bytes — WebGL 2 binds no other (a `vec2` viewport
/// alone failed every pipeline at the start).
#[cfg(test)]
mod webgl2_shaders {
    use wgpu::naga;

    const SHADERS: [(&str, &str); 6] = [
        ("composite", include_str!("shaders/composite.wgsl")),
        ("filter", include_str!("shaders/filter.wgsl")),
        ("image", include_str!("shaders/image.wgsl")),
        ("mipmap", include_str!("shaders/mipmap.wgsl")),
        ("path", include_str!("shaders/path.wgsl")),
        ("quad", include_str!("shaders/quad.wgsl")),
    ];

    #[test]
    fn every_shader_fits_webgl2() {
        for (name, source) in SHADERS {
            let module = naga::front::wgsl::parse_str(source)
                .unwrap_or_else(|err| panic!("{name}: {}", err.emit_to_string(source)));
            for entry in &module.entry_points {
                assert_ne!(
                    entry.stage,
                    naga::ShaderStage::Compute,
                    "{name}: WebGL 2 has no compute"
                );
            }
            for (_, global) in module.global_variables.iter() {
                let label = global.name.as_deref().unwrap_or("?");
                assert!(
                    !matches!(global.space, naga::AddressSpace::Storage { .. }),
                    "{name}: `{label}` is a storage buffer, which WebGL 2 has not"
                );
                if global.space == naga::AddressSpace::Uniform {
                    let size = module.types[global.ty].inner.size(module.to_ctx());
                    assert_eq!(
                        size % 16,
                        0,
                        "{name}: the uniform `{label}` is {size} bytes, not a multiple of 16"
                    );
                }
            }
        }
    }
}
