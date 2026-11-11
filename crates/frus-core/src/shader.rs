// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Custom fragment shaders** (milestone 792): an application writes one WGSL function,
//! frus compiles it once, and it paints rectangles, paths, text, or a whole subtree.
//!
//! # The function to write
//!
//! ```wgsl
//! // Optional: the values the application sets from Rust, by name.
//! struct Uniforms {
//!     tint: vec4<f32>,
//!     speed: f32,
//! };
//!
//! // Required: the colour of one pixel, straight alpha, in sRGB.
//! fn shade(frag: Fragment) -> vec4<f32> {
//!     let wave = 0.5 + 0.5 * sin(frag.uv.x * 12.0 + frag.time * uniforms.speed);
//!     return vec4<f32>(uniforms.tint.rgb * wave, 1.0);
//! }
//! ```
//!
//! frus provides, for the function to use:
//!
//! - `Fragment`: `position` (logical pixels from the box's top-left corner), `uv` (0 to 1
//!   across the box), `size` (the box in logical pixels), `time` (seconds) and
//!   `pixel_ratio` (physical pixels per logical pixel);
//! - `uniforms`: the application's `struct Uniforms`, when it declares one: `f32`, `i32`,
//!   `u32` and `vec2`/`vec3`/`vec4` of `f32` members, set with [`ShaderPaint::float`],
//!   [`ShaderPaint::vec4`], [`ShaderPaint::color`] and the others;
//! - `child(uv)`: the subtree under a shader mask, straight sRGB (transparent elsewhere);
//! - `image0(uv)` to `image3(uv)`: the images given with [`ShaderPaint::image`].
//!
//! The source must declare no bindings and no entry points of its own: frus wraps it in
//! its own (they are appended after it, so the line numbers of an error are the
//! application's). Colours go in and come out as they are written everywhere in frus:
//! straight alpha, sRGB-encoded.
//!
//! # Using it
//!
//! ```
//! use frus_core::{Color, Rect, Scene, Shader};
//!
//! let shader = Shader::from_wgsl(
//!     "struct Uniforms { tint: vec4<f32> };
//!      fn shade(frag: Fragment) -> vec4<f32> {
//!          return vec4<f32>(uniforms.tint.rgb * frag.uv.x, 1.0);
//!      }",
//! )
//! .expect("a valid shader");
//! let paint = shader.paint().color("tint", Color::rgb(1.0, 0.5, 0.0));
//! let mut scene = Scene::new();
//! scene.shader_rect(Rect::new(0.0, 0.0, 200.0, 100.0), &paint);
//! ```
//!
//! [`Shader::from_wgsl`] parses and validates the source once and keeps the result: the
//! same text asks for nothing more, so calling it from a build function costs a hash. The
//! GPU pipeline is made the first time the shader is drawn, and kept.

use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::{Arc, Mutex, OnceLock};

use crate::{BlendMode, Color, ImageHandle, Rect};

/// The frame frus wraps a shader in: entry points, bindings, the helpers the shader calls.
const FRAME: &str = include_str!("shaders/frame.wgsl");

/// The uniform block a shader gets when it declares none.
const NO_UNIFORMS: &str = "\nstruct Uniforms { frus_unused: vec4<f32>, };\n";

/// What a shader that failed to compile paints instead, once its error is said: magenta
/// stripes in a debug build, nothing in a release one.
const PLACEHOLDER: &str = if cfg!(debug_assertions) {
    "fn shade(frag: Fragment) -> vec4<f32> {
    let stripe = step(0.5, fract((frag.position.x + frag.position.y) / 16.0));
    return vec4<f32>(1.0, 0.0, 1.0, 0.85 * stripe);
}
"
} else {
    "fn shade(frag: Fragment) -> vec4<f32> {
    return vec4<f32>(0.0);
}
"
};

/// How many images a shader can sample: `image0` to `image3`.
pub const MAX_SHADER_IMAGES: usize = 4;

/// The type of one member of a shader's `struct Uniforms`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UniformKind {
    /// `f32`.
    Float,
    /// `vec2<f32>`.
    Vec2,
    /// `vec3<f32>`.
    Vec3,
    /// `vec4<f32>`: a colour, too.
    Vec4,
    /// `i32`.
    Int,
    /// `u32`.
    Uint,
}

impl UniformKind {
    /// How many floats it holds; none for the integers.
    pub const fn floats(self) -> usize {
        match self {
            UniformKind::Float => 1,
            UniformKind::Vec2 => 2,
            UniformKind::Vec3 => 3,
            UniformKind::Vec4 => 4,
            UniformKind::Int | UniformKind::Uint => 0,
        }
    }

    /// Its size in bytes.
    pub const fn size(self) -> usize {
        match self {
            UniformKind::Float | UniformKind::Int | UniformKind::Uint => 4,
            UniformKind::Vec2 => 8,
            UniformKind::Vec3 => 12,
            UniformKind::Vec4 => 16,
        }
    }
}

/// One member of a shader's `struct Uniforms`, as the shader declares it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UniformInfo {
    /// Its name in the WGSL source.
    pub name: String,
    /// Its type.
    pub kind: UniformKind,
    /// Where it lies in the uniform block, in bytes, by WGSL's layout rules.
    pub offset: u32,
}

/// A value for one uniform. `From` turns floats, arrays of floats, integers and colours
/// into one.
///
/// ```
/// use frus_core::{Color, UniformValue};
///
/// assert_eq!(UniformValue::from(2.0), UniformValue::Float(2.0));
/// assert_eq!(UniformValue::from(Color::WHITE), UniformValue::Vec4([1.0; 4]));
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UniformValue {
    /// An `f32`.
    Float(f32),
    /// A `vec2<f32>`.
    Vec2([f32; 2]),
    /// A `vec3<f32>`.
    Vec3([f32; 3]),
    /// A `vec4<f32>`.
    Vec4([f32; 4]),
    /// An `i32`.
    Int(i32),
    /// A `u32`.
    Uint(u32),
}

impl UniformValue {
    /// The member type this value fills.
    pub const fn kind(self) -> UniformKind {
        match self {
            UniformValue::Float(_) => UniformKind::Float,
            UniformValue::Vec2(_) => UniformKind::Vec2,
            UniformValue::Vec3(_) => UniformKind::Vec3,
            UniformValue::Vec4(_) => UniformKind::Vec4,
            UniformValue::Int(_) => UniformKind::Int,
            UniformValue::Uint(_) => UniformKind::Uint,
        }
    }

    /// Its bytes, little-endian, as the GPU reads them.
    fn bytes(self) -> Vec<u8> {
        let floats = |v: &[f32]| v.iter().flat_map(|f| f.to_le_bytes()).collect();
        match self {
            UniformValue::Float(v) => v.to_le_bytes().to_vec(),
            UniformValue::Vec2(v) => floats(&v),
            UniformValue::Vec3(v) => floats(&v),
            UniformValue::Vec4(v) => floats(&v),
            UniformValue::Int(v) => v.to_le_bytes().to_vec(),
            UniformValue::Uint(v) => v.to_le_bytes().to_vec(),
        }
    }
}

impl From<f32> for UniformValue {
    fn from(v: f32) -> Self {
        UniformValue::Float(v)
    }
}

impl From<[f32; 2]> for UniformValue {
    fn from(v: [f32; 2]) -> Self {
        UniformValue::Vec2(v)
    }
}

impl From<[f32; 3]> for UniformValue {
    fn from(v: [f32; 3]) -> Self {
        UniformValue::Vec3(v)
    }
}

impl From<[f32; 4]> for UniformValue {
    fn from(v: [f32; 4]) -> Self {
        UniformValue::Vec4(v)
    }
}

impl From<i32> for UniformValue {
    fn from(v: i32) -> Self {
        UniformValue::Int(v)
    }
}

impl From<u32> for UniformValue {
    fn from(v: u32) -> Self {
        UniformValue::Uint(v)
    }
}

/// A colour is a `vec4<f32>` of its sRGB components and its alpha, straight: what `shade`
/// speaks.
impl From<Color> for UniformValue {
    fn from(c: Color) -> Self {
        UniformValue::Vec4(c.to_array())
    }
}

/// Why a uniform or an image could not be set.
#[derive(Clone, Debug, PartialEq)]
pub enum UniformError {
    /// The shader's `struct Uniforms` has no member of that name.
    Unknown {
        /// The name asked for.
        name: String,
    },
    /// The member is of another type.
    WrongType {
        /// The member.
        name: String,
        /// Its type in the shader.
        expected: UniformKind,
        /// The type of the value given.
        given: UniformKind,
    },
    /// There are not that many floats in the block.
    IndexOutOfRange {
        /// The index asked for.
        index: usize,
        /// How many floats the block holds.
        count: usize,
    },
    /// There is no such image slot: they go from 0 to [`MAX_SHADER_IMAGES`] - 1.
    NoSuchImage {
        /// The slot asked for.
        slot: usize,
    },
}

impl fmt::Display for UniformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UniformError::Unknown { name } => {
                write!(f, "the shader's `struct Uniforms` has no member `{name}`")
            }
            UniformError::WrongType {
                name,
                expected,
                given,
            } => write!(f, "uniform `{name}` is a {expected:?}, not a {given:?}"),
            UniformError::IndexOutOfRange { index, count } => {
                write!(f, "float {index} asked for; the uniforms hold {count}")
            }
            UniformError::NoSuchImage { slot } => write!(
                f,
                "image slot {slot}: a shader samples images 0 to {}",
                MAX_SHADER_IMAGES - 1
            ),
        }
    }
}

impl std::error::Error for UniformError {}

/// Why a shader did not compile: where, what, and a report with the offending line.
///
/// ```
/// use frus_core::Shader;
///
/// let err = Shader::from_wgsl(
///     "fn shade(frag: Fragment) -> vec4<f32> {\n    return vec4<f32>(frag.nope);\n}",
/// )
/// .unwrap_err();
/// assert_eq!(err.line, Some(2));
/// assert!(err.to_string().contains("shader.wgsl:2:"));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShaderError {
    /// The line of the application's source the error is on, from 1; `None` when the
    /// error is not on one of its lines (a `shade` missing, say).
    pub line: Option<u32>,
    /// The column on that line, from 1.
    pub column: Option<u32>,
    /// What is wrong, in one line.
    pub message: String,
    /// The full report: the line, a mark under the error, and what the compiler said.
    pub report: String,
}

impl fmt::Display for ShaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.line, self.column) {
            (Some(line), Some(column)) => {
                write!(f, "shader.wgsl:{line}:{column}: {}", self.message)?
            }
            _ => write!(f, "shader.wgsl: {}", self.message)?,
        }
        if !self.report.is_empty() {
            write!(f, "\n{}", self.report)?;
        }
        Ok(())
    }
}

impl std::error::Error for ShaderError {}

/// A compiled shader program: the application's source, wrapped and checked.
struct Program {
    id: u64,
    source: String,
    module: String,
    uniforms: Vec<UniformInfo>,
    /// Where each float of the block lies, in declaration order: what
    /// [`ShaderPaint::float_at`] indexes.
    floats: Vec<u32>,
    /// The block's size, rounded up to 16 bytes, which every GPU binds.
    uniform_size: usize,
}

/// A **fragment shader**, compiled once from WGSL. Cheap to clone; equal when made from the
/// same source.
///
/// ```
/// use frus_core::Shader;
///
/// let a = Shader::from_wgsl("fn shade(f: Fragment) -> vec4<f32> { return vec4<f32>(f.uv, 0.0, 1.0); }")
///     .unwrap();
/// let b = Shader::from_wgsl("fn shade(f: Fragment) -> vec4<f32> { return vec4<f32>(f.uv, 0.0, 1.0); }")
///     .unwrap();
/// assert_eq!(a, b); // compiled once, kept
/// ```
#[derive(Clone)]
pub struct Shader(Arc<Program>);

impl PartialEq for Shader {
    fn eq(&self, other: &Self) -> bool {
        self.0.id == other.0.id
    }
}

impl fmt::Debug for Shader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Shader")
            .field("id", &format_args!("{:016x}", self.0.id))
            .field("uniforms", &self.0.uniforms)
            .finish()
    }
}

/// The shaders compiled so far, and the sources that failed, by the digest of their text.
fn compiled() -> &'static Mutex<HashMap<u64, Result<Shader, ShaderError>>> {
    static CACHE: OnceLock<Mutex<HashMap<u64, Result<Shader, ShaderError>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// FNV-1a, 64 bits.
fn digest(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

impl Shader {
    /// Compiles `source`, a WGSL fragment function `fn shade(frag: Fragment) -> vec4<f32>`
    /// and whatever it needs (see the [module notes](self)), or says where it is wrong.
    ///
    /// The result is kept: the same text again answers the same shader for the cost of a
    /// hash, so this can be called where the paint is built. With `include_str!` and the
    /// hot patching of `frus run`, an edited shader is the new text, and is compiled anew.
    ///
    /// ```
    /// use frus_core::Shader;
    ///
    /// let shader = Shader::from_wgsl(
    ///     "fn shade(frag: Fragment) -> vec4<f32> { return vec4<f32>(frag.uv, 0.5, 1.0); }",
    /// );
    /// assert!(shader.is_ok());
    /// ```
    pub fn from_wgsl(source: &str) -> Result<Shader, ShaderError> {
        let id = digest(source.as_bytes());
        if let Some(known) = compiled().lock().ok().and_then(|c| c.get(&id).cloned()) {
            return known;
        }
        let made = compile(id, source).map(|p| Shader(Arc::new(p)));
        if let Ok(mut cache) = compiled().lock() {
            cache.insert(id, made.clone());
        }
        made
    }

    /// **What to draw when a shader did not compile**: logs `error` (once per error, with
    /// its line) and answers a shader that paints magenta stripes in a debug build and
    /// nothing in a release one, so a typo never stops the application.
    ///
    /// ```
    /// use frus_core::Shader;
    ///
    /// let shader = Shader::from_wgsl("fn shade(").unwrap_or_else(Shader::placeholder);
    /// assert!(shader.uniforms().is_empty());
    /// ```
    pub fn placeholder(error: ShaderError) -> Shader {
        static SAID: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
        let said = SAID.get_or_init(|| Mutex::new(HashSet::new()));
        let text = error.to_string();
        if said
            .lock()
            .map(|mut s| s.insert(text.clone()))
            .unwrap_or(true)
        {
            log::error!("{text}");
        }
        Shader::from_wgsl(PLACEHOLDER).expect("the placeholder compiles")
    }

    /// The digest of its source: the same for the same text.
    pub fn id(&self) -> u64 {
        self.0.id
    }

    /// The members of its `struct Uniforms`, in declaration order.
    pub fn uniforms(&self) -> &[UniformInfo] {
        &self.0.uniforms
    }

    /// How many floats its uniforms hold, as [`ShaderPaint::float_at`] counts them.
    pub fn float_count(&self) -> usize {
        self.0.floats.len()
    }

    /// The size of its uniform block in bytes, rounded up to 16.
    pub fn uniform_size(&self) -> usize {
        self.0.uniform_size
    }

    /// The application's source, as given.
    pub fn source(&self) -> &str {
        &self.0.source
    }

    /// The whole module the GPU compiles: the source and frus's frame after it.
    #[doc(hidden)]
    pub fn module_source(&self) -> &str {
        &self.0.module
    }

    /// A paint of this shader, every uniform at zero and no image, to set and draw with.
    ///
    /// ```
    /// use frus_core::Shader;
    ///
    /// let shader = Shader::from_wgsl(
    ///     "struct Uniforms { speed: f32 };
    ///      fn shade(f: Fragment) -> vec4<f32> { return vec4<f32>(fract(f.time * uniforms.speed)); }",
    /// )
    /// .unwrap();
    /// let paint = shader.paint().float("speed", 2.0).time(1.25);
    /// assert_eq!(paint.seconds(), 1.25);
    /// ```
    pub fn paint(&self) -> ShaderPaint {
        ShaderPaint::new(self)
    }
}

/// `source` without its comments, line breaks kept, so that what it declares can be read.
fn without_comments(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    let mut depth = 0usize;
    while let Some(c) = chars.next() {
        if depth > 0 {
            match (c, chars.peek()) {
                ('*', Some('/')) => {
                    chars.next();
                    depth -= 1;
                }
                ('/', Some('*')) => {
                    chars.next();
                    depth += 1;
                }
                ('\n', _) => out.push('\n'),
                _ => {}
            }
            continue;
        }
        match (c, chars.peek()) {
            ('/', Some('/')) => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            ('/', Some('*')) => {
                chars.next();
                depth = 1;
            }
            _ => out.push(c),
        }
    }
    out
}

/// Whether `source` declares `struct Uniforms`.
fn declares_uniforms(source: &str) -> bool {
    let text = without_comments(source);
    let words: Vec<&str> = text
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|w| !w.is_empty())
        .collect();
    words.windows(2).any(|w| w == ["struct", "Uniforms"])
}

/// Wraps, parses, validates and reflects `source`.
fn compile(id: u64, source: &str) -> Result<Program, ShaderError> {
    let mut module = String::with_capacity(source.len() + FRAME.len() + 64);
    module.push_str(source);
    if !source.ends_with('\n') {
        module.push('\n');
    }
    // Everything past this is frus's.
    let own = module.len();
    if !declares_uniforms(source) {
        module.push_str(NO_UNIFORMS);
    }
    module.push_str(FRAME);

    let parsed = naga::front::wgsl::parse_str(&module).map_err(|e| {
        let at = e.location(&module);
        error_at(
            at.map(|l| (l.offset as usize, l.line_number, l.line_position)),
            own,
            e.message().to_string(),
            e.emit_to_string_with_path(&module, "shader.wgsl"),
        )
    })?;
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        // Nothing beyond what every GPU frus draws on can do, WebGL 2 included.
        naga::valid::Capabilities::empty(),
    )
    .validate(&parsed)
    .map_err(|e| {
        let at = e.location(&module);
        error_at(
            at.map(|l| (l.offset as usize, l.line_number, l.line_position)),
            own,
            e.as_inner().to_string(),
            e.emit_to_string_with_path(&module, "shader.wgsl"),
        )
    })?;

    let (uniforms, size) = reflect(&parsed)?;
    let floats = uniforms
        .iter()
        .flat_map(|u| (0..u.kind.floats()).map(move |i| u.offset + 4 * i as u32))
        .collect();
    Ok(Program {
        id,
        source: source.to_string(),
        module,
        uniforms,
        floats,
        uniform_size: size.div_ceil(16).max(1) * 16,
    })
}

/// An error at byte `offset` of the module: on a line of the application's when it is
/// before `own`, in frus's frame otherwise, which then says what the frame expected.
fn error_at(
    at: Option<(usize, u32, u32)>,
    own: usize,
    message: String,
    report: String,
) -> ShaderError {
    match at {
        Some((offset, line, column)) if offset < own => ShaderError {
            line: Some(line),
            column: Some(column),
            message,
            report,
        },
        _ => {
            let hint = if message.contains("shade") {
                "the shader must define `fn shade(frag: Fragment) -> vec4<f32>`"
            } else if message.contains("Uniforms") || message.contains("uniforms") {
                "`struct Uniforms` must be a struct of f32, i32, u32 and vec2/3/4<f32> members"
            } else {
                "the source clashes with what frus declares around it: declare no bindings, \
                 no entry points, and no names starting with `frus_`, `child` or `image0`-`image3`"
            };
            ShaderError {
                line: None,
                column: None,
                message: format!("{hint} ({message})"),
                report: String::new(),
            }
        }
    }
}

/// The members of `struct Uniforms`, and its size.
fn reflect(module: &naga::Module) -> Result<(Vec<UniformInfo>, usize), ShaderError> {
    let fail = |message: String| ShaderError {
        line: None,
        column: None,
        message,
        report: String::new(),
    };
    let Some((_, var)) = module
        .global_variables
        .iter()
        .find(|(_, g)| g.name.as_deref() == Some("uniforms"))
    else {
        return Err(fail("no uniform block".into()));
    };
    let naga::TypeInner::Struct { members, span } = &module.types[var.ty].inner else {
        return Err(fail("`Uniforms` must be a struct".into()));
    };
    let mut out = Vec::with_capacity(members.len());
    for member in members {
        let name = member.name.clone().unwrap_or_default();
        if name.starts_with("frus_") {
            continue;
        }
        let scalar_kind = |s: &naga::Scalar| match (s.kind, s.width) {
            (naga::ScalarKind::Float, 4) => Some(UniformKind::Float),
            (naga::ScalarKind::Sint, 4) => Some(UniformKind::Int),
            (naga::ScalarKind::Uint, 4) => Some(UniformKind::Uint),
            _ => None,
        };
        let kind = match &module.types[member.ty].inner {
            naga::TypeInner::Scalar(s) => scalar_kind(s),
            naga::TypeInner::Vector { size, scalar }
                if scalar_kind(scalar) == Some(UniformKind::Float) =>
            {
                Some(match size {
                    naga::VectorSize::Bi => UniformKind::Vec2,
                    naga::VectorSize::Tri => UniformKind::Vec3,
                    naga::VectorSize::Quad => UniformKind::Vec4,
                })
            }
            _ => None,
        };
        let Some(kind) = kind else {
            return Err(fail(format!(
                "uniform `{name}`: frus sets f32, i32, u32 and vec2/vec3/vec4<f32> members"
            )));
        };
        out.push(UniformInfo {
            name,
            kind,
            offset: member.offset,
        });
    }
    Ok((out, *span as usize))
}

/// Warns once per shader and message, so a paint rebuilt every frame does not flood the
/// console.
fn warn_once(shader: u64, error: &UniformError) {
    static SAID: OnceLock<Mutex<HashSet<(u64, String)>>> = OnceLock::new();
    let text = error.to_string();
    let said = SAID.get_or_init(|| Mutex::new(HashSet::new()));
    if said
        .lock()
        .map(|mut s| s.insert((shader, text.clone())))
        .unwrap_or(true)
    {
        log::warn!("shader {shader:016x}: {text}");
    }
}

/// **A shader with its values**: the uniforms, the images, the time and how it meets what
/// it paints. Built from [`Shader::paint`], drawn with [`crate::Scene::shader_rect`] and the
/// other `shader_*` methods of a scene, or by the widgets that take one.
///
/// The setters are builders that never fail: a name the shader lacks or a value of the
/// wrong type is said once in the console and ignored. [`ShaderPaint::set`] answers the
/// error instead.
///
/// ```
/// use frus_core::{BlendMode, Color, Shader};
///
/// let shader = Shader::from_wgsl(
///     "struct Uniforms { tint: vec4<f32>, size: vec2<f32>, speed: f32 };
///      fn shade(f: Fragment) -> vec4<f32> { return uniforms.tint; }",
/// )
/// .unwrap();
/// let mut paint = shader
///     .paint()
///     .color("tint", Color::rgb(0.2, 0.4, 1.0))
///     .vec2("size", [10.0, 20.0])
///     .float("speed", 3.0)
///     .blend(BlendMode::Modulate);
/// assert!(paint.set("missing", 1.0).is_err());
/// ```
#[derive(Clone, Debug)]
pub struct ShaderPaint {
    shader: Shader,
    uniforms: Vec<u8>,
    images: [Option<ImageHandle>; MAX_SHADER_IMAGES],
    time: f32,
    blend: Option<BlendMode>,
    /// The box the shader spans, in the scene's pixels.
    rect: Rect,
    /// The same box in logical pixels, as it was painted: what `Fragment` reports.
    logical: (f32, f32),
}

impl PartialEq for ShaderPaint {
    fn eq(&self, other: &Self) -> bool {
        let image_id = |i: &Option<ImageHandle>| i.as_ref().map(|i| i.id());
        self.shader == other.shader
            && self.uniforms == other.uniforms
            && self
                .images
                .iter()
                .map(image_id)
                .eq(other.images.iter().map(image_id))
            && self.time == other.time
            && self.blend == other.blend
            && self.rect == other.rect
            && self.logical == other.logical
    }
}

impl ShaderPaint {
    /// A paint of `shader`, every uniform at zero.
    pub fn new(shader: &Shader) -> ShaderPaint {
        ShaderPaint {
            shader: shader.clone(),
            uniforms: vec![0; shader.uniform_size()],
            images: Default::default(),
            time: 0.0,
            blend: None,
            rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            logical: (0.0, 0.0),
        }
    }

    /// Sets the uniform `name`, or says why it cannot.
    ///
    /// ```
    /// use frus_core::{Shader, UniformError};
    ///
    /// let shader = Shader::from_wgsl(
    ///     "struct Uniforms { speed: f32 }; fn shade(f: Fragment) -> vec4<f32> { return vec4<f32>(uniforms.speed); }",
    /// )
    /// .unwrap();
    /// let mut paint = shader.paint();
    /// assert_eq!(paint.set("speed", 2.0), Ok(()));
    /// assert!(matches!(paint.set("speed", [1.0, 2.0]), Err(UniformError::WrongType { .. })));
    /// ```
    pub fn set(&mut self, name: &str, value: impl Into<UniformValue>) -> Result<(), UniformError> {
        let value = value.into();
        let Some(info) = self.shader.uniforms().iter().find(|u| u.name == name) else {
            return Err(UniformError::Unknown { name: name.into() });
        };
        if info.kind != value.kind() {
            return Err(UniformError::WrongType {
                name: name.into(),
                expected: info.kind,
                given: value.kind(),
            });
        }
        let at = info.offset as usize;
        let bytes = value.bytes();
        self.uniforms[at..at + bytes.len()].copy_from_slice(&bytes);
        Ok(())
    }

    /// Sets the `index`-th float of the uniforms, counting every component of every float
    /// member in declaration order: a `vec2` then an `f32` are floats 0, 1 and 2.
    ///
    /// ```
    /// use frus_core::Shader;
    ///
    /// let shader = Shader::from_wgsl(
    ///     "struct Uniforms { size: vec2<f32>, speed: f32 };
    ///      fn shade(f: Fragment) -> vec4<f32> { return vec4<f32>(uniforms.speed); }",
    /// )
    /// .unwrap();
    /// let mut paint = shader.paint();
    /// assert!(paint.set_float_at(2, 3.0).is_ok());
    /// assert!(paint.set_float_at(3, 3.0).is_err());
    /// ```
    pub fn set_float_at(&mut self, index: usize, value: f32) -> Result<(), UniformError> {
        let floats = &self.shader.0.floats;
        let Some(&at) = floats.get(index) else {
            return Err(UniformError::IndexOutOfRange {
                index,
                count: floats.len(),
            });
        };
        let at = at as usize;
        self.uniforms[at..at + 4].copy_from_slice(&value.to_le_bytes());
        Ok(())
    }

    /// Gives `image` to the shader as `image{slot}(uv)`, or says the slot does not exist.
    pub fn set_image(&mut self, slot: usize, image: &ImageHandle) -> Result<(), UniformError> {
        let Some(place) = self.images.get_mut(slot) else {
            return Err(UniformError::NoSuchImage { slot });
        };
        *place = Some(image.clone());
        Ok(())
    }

    fn setting(mut self, name: &str, value: UniformValue) -> Self {
        if let Err(e) = self.set(name, value) {
            warn_once(self.shader.id(), &e);
        }
        self
    }

    /// Sets the `f32` uniform `name`.
    pub fn float(self, name: &str, value: f32) -> Self {
        self.setting(name, UniformValue::Float(value))
    }

    /// Sets the `vec2<f32>` uniform `name`.
    pub fn vec2(self, name: &str, value: [f32; 2]) -> Self {
        self.setting(name, UniformValue::Vec2(value))
    }

    /// Sets the `vec3<f32>` uniform `name`.
    pub fn vec3(self, name: &str, value: [f32; 3]) -> Self {
        self.setting(name, UniformValue::Vec3(value))
    }

    /// Sets the `vec4<f32>` uniform `name`.
    pub fn vec4(self, name: &str, value: [f32; 4]) -> Self {
        self.setting(name, UniformValue::Vec4(value))
    }

    /// Sets the `vec4<f32>` uniform `name` to a colour: its sRGB components and its alpha,
    /// straight, as `shade` answers them.
    pub fn color(self, name: &str, color: Color) -> Self {
        self.setting(name, color.into())
    }

    /// Sets the `i32` uniform `name`.
    pub fn int(self, name: &str, value: i32) -> Self {
        self.setting(name, UniformValue::Int(value))
    }

    /// Sets the `u32` uniform `name`.
    pub fn uint(self, name: &str, value: u32) -> Self {
        self.setting(name, UniformValue::Uint(value))
    }

    /// Sets the `index`-th float of the uniforms: see [`ShaderPaint::set_float_at`].
    pub fn float_at(mut self, index: usize, value: f32) -> Self {
        if let Err(e) = self.set_float_at(index, value) {
            warn_once(self.shader.id(), &e);
        }
        self
    }

    /// Gives `image` to the shader as `image{slot}(uv)`, slot 0 to 3.
    pub fn image(mut self, slot: usize, image: &ImageHandle) -> Self {
        if let Err(e) = self.set_image(slot, image) {
            warn_once(self.shader.id(), &e);
        }
        self
    }

    /// The time `Fragment::time` reports, in seconds. An `AnimatedShader` sets it from
    /// the clock, frame by frame.
    pub fn time(mut self, seconds: f32) -> Self {
        self.time = seconds;
        self
    }

    /// How the shader's colour meets what it paints, as the compositor blends it: the
    /// shader is the source, what is under it in the group (the child of a mask, the
    /// shape or text it colours) the destination. Unset, a fill replaces
    /// ([`BlendMode::Src`]), a shape or text is coloured where it is painted
    /// ([`BlendMode::SrcIn`]) and a mask multiplies ([`BlendMode::Modulate`]).
    pub fn blend(mut self, mode: BlendMode) -> Self {
        self.blend = Some(mode);
        self
    }

    /// Spans the shader over `rect`, in logical pixels: what `Fragment::uv` runs across.
    /// The scene's `shader_*` methods set it from what they paint.
    pub fn at(mut self, rect: Rect) -> Self {
        self.rect = rect;
        self.logical = (rect.width, rect.height);
        self
    }

    /// The blend mode asked for, `None` for the default of where it is used.
    pub fn blend_mode(&self) -> Option<BlendMode> {
        self.blend
    }

    /// The blend mode it is drawn with: the one asked for, or `default`.
    pub fn blend_or(&self, default: BlendMode) -> BlendMode {
        self.blend.unwrap_or(default)
    }

    /// The shader.
    pub fn shader(&self) -> &Shader {
        &self.shader
    }

    /// The uniform block's bytes, as the GPU reads them.
    pub fn uniform_bytes(&self) -> &[u8] {
        &self.uniforms
    }

    /// The images, by slot.
    pub fn images(&self) -> &[Option<ImageHandle>; MAX_SHADER_IMAGES] {
        &self.images
    }

    /// The time `Fragment::time` reports.
    pub fn seconds(&self) -> f32 {
        self.time
    }

    /// The box the shader spans, in the scene's pixels (physical, once a scene is scaled
    /// for the display).
    pub fn rect(&self) -> Rect {
        self.rect
    }

    /// Physical pixels per logical pixel: the box now over the box as it was painted.
    pub fn pixel_ratio(&self) -> f32 {
        if self.logical.0 > 0.0 {
            self.rect.width / self.logical.0
        } else {
            1.0
        }
    }

    /// Follows a scaling of the scene, as DPI does: the box grows, the logical size stays.
    pub fn scaled_xy(mut self, sx: f32, sy: f32) -> ShaderPaint {
        self.rect = self.rect.scale_xy(sx, sy);
        self
    }

    /// Moves the box with the subtree it paints.
    pub fn translated(mut self, dx: f32, dy: f32) -> ShaderPaint {
        self.rect = self.rect.translate(dx, dy);
        self
    }
}

/// **A picture an application draws on the GPU itself**, carried by a scene to the
/// renderer: the box it fills and what draws it, which only the renderer can read
/// (`frus_gpu::GpuCallback`). Applications use the `GpuPass` widget rather than this.
#[derive(Clone)]
pub struct ExternalPass {
    key: u64,
    rect: Rect,
    logical: (f32, f32),
    time: f32,
    payload: Arc<dyn Any + Send + Sync>,
}

impl fmt::Debug for ExternalPass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExternalPass")
            .field("key", &self.key)
            .field("rect", &self.rect)
            .field("time", &self.time)
            .finish_non_exhaustive()
    }
}

impl PartialEq for ExternalPass {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
            && self.rect == other.rect
            && self.logical == other.logical
            && self.time == other.time
            && std::ptr::eq(
                Arc::as_ptr(&self.payload) as *const (),
                Arc::as_ptr(&other.payload) as *const (),
            )
    }
}

impl ExternalPass {
    /// A pass filling `rect` (logical pixels), drawn by `payload`, kept apart from the
    /// others by `key`: the renderer keeps one texture and one state per key.
    ///
    /// ```
    /// use std::sync::Arc;
    /// use frus_core::{ExternalPass, Rect};
    ///
    /// let pass = ExternalPass::new(7, Rect::new(0.0, 0.0, 64.0, 32.0), Arc::new(()));
    /// assert_eq!(pass.key(), 7);
    /// ```
    pub fn new(key: u64, rect: Rect, payload: Arc<dyn Any + Send + Sync>) -> ExternalPass {
        ExternalPass {
            key,
            rect,
            logical: (rect.width, rect.height),
            time: 0.0,
            payload,
        }
    }

    /// The time it is drawn at, in seconds.
    pub fn time(mut self, seconds: f32) -> ExternalPass {
        self.time = seconds;
        self
    }

    /// Which pass this is, from one frame to the next.
    pub fn key(&self) -> u64 {
        self.key
    }

    /// The box it fills, in the scene's pixels.
    pub fn rect(&self) -> Rect {
        self.rect
    }

    /// The time it is drawn at.
    pub fn seconds(&self) -> f32 {
        self.time
    }

    /// Physical pixels per logical pixel.
    pub fn pixel_ratio(&self) -> f32 {
        if self.logical.0 > 0.0 {
            self.rect.width / self.logical.0
        } else {
            1.0
        }
    }

    /// What draws it, for the renderer to read.
    pub fn payload(&self) -> &(dyn Any + Send + Sync) {
        &*self.payload
    }

    /// Follows a scaling of the scene.
    pub fn scaled_xy(mut self, sx: f32, sy: f32) -> ExternalPass {
        self.rect = self.rect.scale_xy(sx, sy);
        self
    }

    /// Moves the box with the subtree.
    pub fn translated(mut self, dx: f32, dy: f32) -> ExternalPass {
        self.rect = self.rect.translate(dx, dy);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRADIENT: &str = "struct Uniforms {
    start: vec4<f32>,
    end: vec4<f32>,
    angle: f32,
    center: vec2<f32>,
    count: i32,
};

fn shade(frag: Fragment) -> vec4<f32> {
    return mix(uniforms.start, uniforms.end, frag.uv.x);
}
";

    /// The members are read from the source with WGSL's own layout: a `vec2` after an
    /// `f32` starts on the next 8 bytes, and the block is rounded up to 16.
    #[test]
    fn the_uniform_layout_is_the_shaders_own() {
        let shader = Shader::from_wgsl(GRADIENT).unwrap();
        let layout: Vec<(&str, UniformKind, u32)> = shader
            .uniforms()
            .iter()
            .map(|u| (u.name.as_str(), u.kind, u.offset))
            .collect();
        assert_eq!(
            layout,
            [
                ("start", UniformKind::Vec4, 0),
                ("end", UniformKind::Vec4, 16),
                ("angle", UniformKind::Float, 32),
                ("center", UniformKind::Vec2, 40),
                ("count", UniformKind::Int, 48),
            ]
        );
        assert_eq!(shader.uniform_size(), 64);
        // start (4), end (4), angle (1), center (2): the integer is not a float.
        assert_eq!(shader.float_count(), 11);
    }

    /// Values land at their member's offset, little-endian, and nowhere else.
    #[test]
    fn values_are_packed_at_their_offsets() {
        let shader = Shader::from_wgsl(GRADIENT).unwrap();
        let paint = shader
            .paint()
            .color("end", Color::rgba(0.25, 0.5, 0.75, 1.0))
            .float("angle", 2.0)
            .vec2("center", [3.0, 4.0])
            .int("count", -5);
        let bytes = paint.uniform_bytes();
        let f = |at: usize| f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        assert_eq!([f(0), f(4), f(8), f(12)], [0.0; 4]);
        assert_eq!([f(16), f(20), f(24), f(28)], [0.25, 0.5, 0.75, 1.0]);
        assert_eq!(f(32), 2.0);
        assert_eq!(f(36), 0.0, "the padding before the vec2 is left alone");
        assert_eq!([f(40), f(44)], [3.0, 4.0]);
        assert_eq!(i32::from_le_bytes(bytes[48..52].try_into().unwrap()), -5);
        // Float 8 is `angle`, float 9 the vec2's first component.
        let by_index = shader.paint().float_at(8, 7.0).float_at(9, 8.0);
        let b = by_index.uniform_bytes();
        assert_eq!(f32::from_le_bytes(b[32..36].try_into().unwrap()), 7.0);
        assert_eq!(f32::from_le_bytes(b[40..44].try_into().unwrap()), 8.0);
    }

    #[test]
    fn a_wrong_name_or_type_is_refused_and_changes_nothing() {
        let shader = Shader::from_wgsl(GRADIENT).unwrap();
        let mut paint = shader.paint();
        assert_eq!(
            paint.set("nope", 1.0),
            Err(UniformError::Unknown {
                name: "nope".into()
            })
        );
        assert_eq!(
            paint.set("angle", [1.0, 2.0]),
            Err(UniformError::WrongType {
                name: "angle".into(),
                expected: UniformKind::Float,
                given: UniformKind::Vec2,
            })
        );
        assert!(matches!(
            paint.set_float_at(11, 1.0),
            Err(UniformError::IndexOutOfRange {
                index: 11,
                count: 11
            })
        ));
        let img = crate::ImageData::from_rgba(1, 1, vec![0; 4]).into_handle();
        assert_eq!(
            paint.set_image(4, &img),
            Err(UniformError::NoSuchImage { slot: 4 })
        );
        // The builders say so and go on.
        let built = shader.paint().float("nope", 1.0).image(9, &img);
        assert_eq!(built, shader.paint());
        assert!(paint.uniform_bytes().iter().all(|&b| b == 0));
    }

    /// A shader with no `struct Uniforms` gets an empty one of 16 bytes, which every GPU
    /// binds.
    #[test]
    fn no_uniforms_is_sixteen_bytes_and_no_member() {
        let shader =
            Shader::from_wgsl("fn shade(f: Fragment) -> vec4<f32> { return vec4<f32>(1.0); }")
                .unwrap();
        assert!(shader.uniforms().is_empty());
        assert_eq!(shader.uniform_size(), 16);
    }

    /// `struct Uniforms` in a comment is not a declaration.
    #[test]
    fn a_commented_uniforms_block_does_not_count() {
        assert!(!declares_uniforms("// struct Uniforms { a: f32 }\n"));
        assert!(!declares_uniforms("/* struct /* nested */ Uniforms */"));
        assert!(declares_uniforms("struct\n  Uniforms {a: f32}"));
        assert!(!declares_uniforms("struct UniformsX {a: f32}"));
    }

    /// The same source is compiled once: a second call is the same program.
    #[test]
    fn the_same_source_is_compiled_once() {
        let src = "fn shade(f: Fragment) -> vec4<f32> { return vec4<f32>(f.uv, 0.25, 1.0); }";
        let a = Shader::from_wgsl(src).unwrap();
        let b = Shader::from_wgsl(src).unwrap();
        assert!(Arc::ptr_eq(&a.0, &b.0));
        // Another text is another shader: what a hot patch brings.
        let c = Shader::from_wgsl(&src.replace("0.25", "0.5")).unwrap();
        assert_ne!(a, c);
    }

    /// An error on one of the application's lines names that line and column, and the
    /// report shows the line.
    #[test]
    fn an_error_names_the_applications_line() {
        let src = "struct Uniforms { speed: f32 };\n\nfn shade(frag: Fragment) -> vec4<f32> {\n    let x = frag.speed;\n    return vec4<f32>(x);\n}\n";
        let err = Shader::from_wgsl(src).unwrap_err();
        assert_eq!(err.line, Some(4), "{err}");
        assert!(err.column.is_some());
        assert!(err.report.contains("frag.speed"), "{}", err.report);
        assert!(err.to_string().starts_with("shader.wgsl:4:"), "{err}");
        // A type error, found by validation rather than by the parser.
        let err = Shader::from_wgsl(
            "fn shade(frag: Fragment) -> vec4<f32> {\n    let a: f32 = vec2<f32>(1.0);\n    return vec4<f32>(a);\n}",
        )
        .unwrap_err();
        assert_eq!(err.line, Some(2), "{err}");
    }

    /// A missing `shade` is said in words, not as a line of frus's frame.
    #[test]
    fn a_missing_shade_is_explained() {
        let err =
            Shader::from_wgsl("fn paint(f: Fragment) -> vec4<f32> { return vec4<f32>(1.0); }")
                .unwrap_err();
        assert_eq!(err.line, None);
        assert!(err.message.contains("fn shade(frag: Fragment)"), "{err}");
        let err = Shader::from_wgsl("fn shade(f: Fragment) -> f32 { return 1.0; }").unwrap_err();
        assert_eq!(err.line, None, "{err}");
    }

    /// A member of a type frus cannot set is refused when the shader is made, not when
    /// it is drawn.
    #[test]
    fn an_unsettable_uniform_is_refused() {
        let err = Shader::from_wgsl(
            "struct Uniforms { m: mat4x4<f32> };\nfn shade(f: Fragment) -> vec4<f32> { return uniforms.m[0]; }",
        )
        .unwrap_err();
        assert!(err.message.contains("uniform `m`"), "{err}");
    }

    /// The placeholder compiles, and says nothing is wrong with it.
    #[test]
    fn the_placeholder_draws() {
        let err = Shader::from_wgsl("fn shade(").unwrap_err();
        let shader = Shader::placeholder(err);
        assert!(shader.source().contains("fn shade"));
        assert_eq!(shader.uniform_size(), 16);
    }

    #[test]
    fn a_paint_follows_the_scene() {
        let shader =
            Shader::from_wgsl("fn shade(f: Fragment) -> vec4<f32> { return vec4<f32>(1.0); }")
                .unwrap();
        let paint = shader
            .paint()
            .at(Rect::new(10.0, 20.0, 100.0, 50.0))
            .scaled_xy(2.0, 2.0)
            .translated(1.0, 1.0);
        assert_eq!(paint.rect(), Rect::new(21.0, 41.0, 200.0, 100.0));
        assert_eq!(paint.pixel_ratio(), 2.0);
    }
}
