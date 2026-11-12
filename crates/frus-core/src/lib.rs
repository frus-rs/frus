// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! `frus-core` — the fundamental types shared by the whole framework.
//!
//! This crate holds **no logic**: only data types (geometry, colour) with no
//! dependency on rendering or on any platform. It is the common foundation for
//! `frus-gpu`, `frus-layout` and the rest, which keeps those layers from
//! duplicating types or coupling to one another.

#![deny(missing_docs)]

// The README's example is compiled and run by `cargo test --doc`, so it cannot rot.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
pub struct ReadmeDoctests;

pub mod animation;
mod color;
mod colors;
mod decoration;
mod decoration_image;
mod device;
mod filter;
mod geometry;
mod gradient;
mod hct;
mod image;
mod path;
mod platform;
mod responsive;
mod scene;
mod semantics;
mod shader;
mod shape;
mod text_shaping;
mod text_style;
mod texture;

pub use animation::{
    animation_speed, dilate_frame_time, set_animation_speed, Animatable, Animation,
    AnimationController, BouncingScrollSimulation, ClampedSimulation, ClampingScrollSimulation,
    Curve, Curved, FrictionSimulation, Lerp, PolynomialFit, Simulation, SpringDescription,
    SpringSimulation, Status, Tolerance, Tween, TweenSequence, Velocity, VelocityEstimate,
    VelocityStrategy, VelocityTracker, BOUNCING_DRAG, BOUNCING_FLING_WEIGHTS, CLAMPING_FRICTION,
    DESKTOP_FLING_WEIGHTS, MAX_ANIMATION_SPEED, MAX_SPRING_TRANSFER_VELOCITY, MIN_ANIMATION_SPEED,
};
pub use color::{surface_tint_opacity, Color};
pub use colors::{Colors, MaterialColor};
pub use decoration::{
    paint_elevation, Border, BorderDirectional, BorderRadius, BorderRadiusDirectional, BoxBorder,
    BoxDecoration, BoxShadow, BoxShape,
};
pub use decoration_image::{
    apply_box_fit, DecorationImage, FittedSizes, ImagePiece, ImageRepeat, MAX_IMAGE_TILES,
};
pub use device::{ButtonEdge, Cutout, DeviceFrame, DeviceInsets, DeviceProfile, SideButton};
pub use filter::{
    Backdrop, BlendMode, ColorFilter, FractionalMask, ImageFilter, LayerFilter, MaskShader,
    ShaderMask,
};
pub use geometry::{
    fits, Affine, Alignment, AlignmentDirectional, AlignmentGeometry, Insets, InsetsDirectional,
    InsetsGeometry, Point, Rect, Size, TextDirection, WindowInsets,
};
pub use gradient::{tile, Gradient, LinearGradient, RadialGradient, SweepGradient, TileMode};
pub use hct::{Hct, TonalPalette};
pub use image::{
    advance_image_frame, cached, cached_at, clear_fetched_images, decoded_at, fetched, fetched_at,
    forget_cached_images, forget_fetched_image, forget_fetched_images, image_cache_budget,
    image_cache_bytes, image_fetcher, image_interest, image_landings, image_seen, image_wanted,
    images_in_flight, set_image_cache_budget, set_image_fetcher, trim_image_caches, BoxFit,
    DecodeSize, Fetched, FilterQuality, ImageData, ImageFetcher, ImageHandle, ImageInterest,
    DEFAULT_IMAGE_CACHE_BYTES, IMAGE_FETCH_CANCELLED,
};
pub use path::{FillRule, Path, PathVerb, Stroke, StrokeCap};
pub use platform::{
    __set_detected_target_platform, debug_default_target_platform_override,
    default_target_platform, set_debug_default_target_platform_override, Platform, TargetPlatform,
    IS_WEB,
};
pub use responsive::{Orientation, SizeClass};
pub use scene::{
    BlurStyle, ClipShape, GradientRamp, LayerTransform, PathGradient, Primitive, RampShape, Scene,
    TextBlock,
};
pub use semantics::{Role, SemanticsProperties, Toggled};
pub use shader::{
    ExternalPass, Shader, ShaderError, ShaderPaint, UniformError, UniformInfo, UniformKind,
    UniformValue, MAX_SHADER_IMAGES,
};
pub use shape::{BorderSide, ShapeBorder};
pub use text_shaping::{FontTag, TextShaping};
pub use text_style::{
    install_text_scale, text_scale, with_text_scale, FontFamily, FontWeight, ResolvedTextStyle,
    TextAffinity, TextAlign, TextDecoration, TextOverflow, TextPosition, TextRun, TextScaleGuard,
    TextSpan, TextStyle, DEFAULT_LINE_HEIGHT, DEFAULT_TEXT_SIZE,
};
pub use texture::{
    textures, ExternalTexture, FrameInfo, FramePlane, HardwareBuffer, PixelFormat, TextureError,
    TextureFrame, TextureId, TextureRegistry, TextureStats, YuvColorSpace, YuvPlane,
    MAX_TEXTURE_SIDE,
};
