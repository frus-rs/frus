
// ---------------------------------------------------------------------------------------
// frus: the frame around an application's fragment shader (milestone 792).
//
// This text is appended after the application's own source, so the line numbers of an
// error in that source are its own. WGSL declarations at module scope may come in any
// order, which is what lets the application use `Fragment`, `child`, `image0`... before
// they are declared here.
//
// The application writes one function, `fn shade(frag: Fragment) -> vec4<f32>`, which
// answers a straight-alpha colour in sRGB, the way colours are written everywhere else in
// frus. Everything here is named `frus_*`, apart from what the application uses.
// ---------------------------------------------------------------------------------------

// What `shade` is told about the pixel it colours.
struct Fragment {
    // Logical pixels from the top-left corner of the painted box.
    position: vec2<f32>,
    // `position / size`: 0 to 1 across the box.
    uv: vec2<f32>,
    // The box, in logical pixels.
    size: vec2<f32>,
    // Seconds: the paint's time, or the animation's.
    time: f32,
    // Physical pixels per logical pixel.
    pixel_ratio: f32,
};

struct FrusViewport {
    size: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0)
var<uniform> frus_viewport: FrusViewport;

// The child under a shader mask, as composited layers are: premultiplied linear light.
@group(1) @binding(0)
var frus_child_tex: texture_2d<f32>;
@group(1) @binding(1)
var frus_child_samp: sampler;
// The clip mask of a path clip; opaque white otherwise.
@group(1) @binding(2)
var frus_clip_mask: texture_2d<f32>;

struct FrusShaderFrame {
    // The painted box on the target, in physical pixels, before any layer transform.
    rect: vec4<f32>,
    // Where the child's texture lies on the target.
    region: vec4<f32>,
    // (time, pixel ratio, blend mode, flags): flag 1 a child is there, flag 2 the colour
    // comes from an external texture in `frus_image0` (a GPU pass), not from `shade`.
    info: vec4<f32>,
    _pad: vec4<f32>,
};

@group(2) @binding(0)
var<uniform> frus_frame: FrusShaderFrame;
// The application's own uniforms, as its `struct Uniforms` declares them.
@group(2) @binding(1)
var<uniform> uniforms: Uniforms;
@group(2) @binding(2)
var frus_image0: texture_2d<f32>;
@group(2) @binding(3)
var frus_image1: texture_2d<f32>;
@group(2) @binding(4)
var frus_image2: texture_2d<f32>;
@group(2) @binding(5)
var frus_image3: texture_2d<f32>;
@group(2) @binding(6)
var frus_image_samp: sampler;

struct FrusVertexInput {
    @location(0) unit_pos: vec2<f32>,
};

struct FrusInstanceInput {
    @location(1) clip: vec4<f32>,
    @location(2) inv_lin: vec4<f32>,
    @location(3) inv_tr_op: vec4<f32>,
    @location(4) shape: vec4<f32>,
    @location(5) radii: vec4<f32>,
    @location(6) region: vec4<f32>,
};

struct FrusVaryings {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) frag_px: vec2<f32>,
    @location(2) @interpolate(flat) clip: vec4<f32>,
    @location(3) @interpolate(flat) opacity: f32,
    @location(4) @interpolate(flat) inv_lin: vec4<f32>,
    @location(5) @interpolate(flat) inv_tr: vec2<f32>,
    @location(6) @interpolate(flat) shape: vec4<f32>,
    @location(7) @interpolate(flat) radii: vec4<f32>,
};

@vertex
fn frus_vs(vert: FrusVertexInput, inst: FrusInstanceInput) -> FrusVaryings {
    let px = inst.region.xy + vert.unit_pos * inst.region.zw;
    let unit = px / frus_viewport.size;
    var out: FrusVaryings;
    out.clip_position = vec4<f32>(unit.x * 2.0 - 1.0, 1.0 - unit.y * 2.0, 0.0, 1.0);
    out.uv = unit;
    out.frag_px = px;
    out.clip = inst.clip;
    out.opacity = inst.inv_tr_op.z;
    out.inv_lin = inst.inv_lin;
    out.inv_tr = inst.inv_tr_op.xy;
    out.shape = inst.shape;
    out.radii = inst.radii;
    return out;
}

// Premultiplied linear light, as textures hold it, to straight sRGB, as `shade` speaks.
fn frus_to_straight_srgb(c: vec4<f32>) -> vec4<f32> {
    let straight = select(c.rgb / c.a, vec3<f32>(0.0), c.a <= 0.0);
    return vec4<f32>(frus_linear_to_srgb(clamp(straight, vec3<f32>(0.0), vec3<f32>(1.0))), c.a);
}

fn frus_flags() -> u32 {
    return u32(frus_frame.info.w + 0.5);
}

// The child at a target pixel, premultiplied linear; transparent outside it, or when
// there is none.
fn frus_child_at(px: vec2<f32>) -> vec4<f32> {
    let uv = (px - frus_frame.region.xy) / max(frus_frame.region.zw, vec2<f32>(1.0));
    let sample = textureSampleLevel(frus_child_tex, frus_child_samp, uv, 0.0);
    let inside = all(uv >= vec2<f32>(0.0)) && all(uv <= vec2<f32>(1.0))
        && (frus_flags() & 1u) != 0u;
    return select(vec4<f32>(0.0), sample, inside);
}

// The child under a shader mask at `uv` of the painted box, straight sRGB: transparent
// outside it, and everywhere when the shader paints no child.
fn child(uv: vec2<f32>) -> vec4<f32> {
    let px = frus_frame.rect.xy + uv * frus_frame.rect.zw;
    return frus_to_straight_srgb(frus_child_at(px));
}

// An image the paint was given, at `uv` (0 to 1 across the image), straight sRGB.
// Transparent black where no image was given.
fn image0(uv: vec2<f32>) -> vec4<f32> {
    let t = textureSampleLevel(frus_image0, frus_image_samp, uv, 0.0);
    return vec4<f32>(frus_linear_to_srgb(t.rgb), t.a);
}

fn image1(uv: vec2<f32>) -> vec4<f32> {
    let t = textureSampleLevel(frus_image1, frus_image_samp, uv, 0.0);
    return vec4<f32>(frus_linear_to_srgb(t.rgb), t.a);
}

fn image2(uv: vec2<f32>) -> vec4<f32> {
    let t = textureSampleLevel(frus_image2, frus_image_samp, uv, 0.0);
    return vec4<f32>(frus_linear_to_srgb(t.rgb), t.a);
}

fn image3(uv: vec2<f32>) -> vec4<f32> {
    let t = textureSampleLevel(frus_image3, frus_image_samp, uv, 0.0);
    return vec4<f32>(frus_linear_to_srgb(t.rgb), t.a);
}

fn frus_sd_rounded_box(p: vec2<f32>, half: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - half + vec2<f32>(r, r);
    return length(max(q, vec2<f32>(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - r;
}

fn frus_corner_radius(p: vec2<f32>, radii: vec4<f32>) -> f32 {
    if (p.x < 0.0) {
        return select(radii.w, radii.x, p.y < 0.0);
    }
    return select(radii.z, radii.y, p.y < 0.0);
}

// The layer's clip shape at a target pixel, as the compositor reads it.
fn frus_clip_coverage(in: FrusVaryings) -> f32 {
    let center = in.clip.xy + in.clip.zw * 0.5;
    let half = in.clip.zw * 0.5;
    let q = in.frag_px - center;
    let kind = in.shape.x;
    let in_rect = f32(
        in.frag_px.x >= in.clip.x
        && in.frag_px.x <= in.clip.x + in.clip.z
        && in.frag_px.y >= in.clip.y
        && in.frag_px.y <= in.clip.y + in.clip.w
    );
    if (kind > 0.5 && kind < 1.5) {
        let r = min(frus_corner_radius(q, in.radii), min(half.x, half.y));
        return 1.0 - smoothstep(-0.5, 0.5, frus_sd_rounded_box(q, half, r));
    }
    if (kind > 1.5 && kind < 2.5) {
        let e = q / max(half, vec2<f32>(1.0, 1.0));
        let d = (length(e) - 1.0) * min(half.x, half.y);
        return 1.0 - smoothstep(-0.5, 0.5, d);
    }
    return in_rect;
}

@fragment
fn frus_fs(in: FrusVaryings) -> @location(0) vec4<f32> {
    // Under a layer transform the box is painted flat and the target pixel is carried
    // back into it, as the compositor does for every layer.
    let p = in.frag_px;
    let src_px = vec2<f32>(
        in.inv_lin.x * p.x + in.inv_lin.z * p.y + in.inv_tr.x,
        in.inv_lin.y * p.x + in.inv_lin.w * p.y + in.inv_tr.y,
    );
    let rect = frus_frame.rect;
    let local = src_px - rect.xy;
    let inside = f32(all(local >= vec2<f32>(0.0)) && all(local <= rect.zw));
    let ratio = max(frus_frame.info.y, 1e-6);
    var frag: Fragment;
    frag.position = local / ratio;
    frag.size = rect.zw / ratio;
    frag.uv = local / max(rect.zw, vec2<f32>(1e-6));
    frag.time = frus_frame.info.x;
    frag.pixel_ratio = ratio;
    let mask_a = textureSampleLevel(frus_clip_mask, frus_child_samp, in.uv, 0.0).a;
    let cover = in.opacity * frus_clip_coverage(in) * mask_a * inside;
    if ((frus_flags() & 2u) != 0u) {
        // A GPU pass: its texture is premultiplied linear light already.
        return textureSampleLevel(frus_image0, frus_image_samp, frag.uv, 0.0) * cover;
    }
    let s = clamp(shade(frag), vec4<f32>(0.0), vec4<f32>(1.0));
    // Both sides premultiplied and sRGB-encoded, as the compositor blends a mask.
    let src = vec4<f32>(s.rgb * s.a, s.a);
    let c = frus_to_straight_srgb(frus_child_at(src_px));
    let dst = vec4<f32>(c.rgb * c.a, c.a);
    let r = frus_blend(src, dst, u32(frus_frame.info.z + 0.5));
    let a = clamp(r.a, 0.0, 1.0);
    let straight = select(r.rgb / a, vec3<f32>(0.0), a <= 0.0);
    let lin = frus_srgb_to_linear(clamp(straight, vec3<f32>(0.0), vec3<f32>(1.0)));
    return vec4<f32>(lin * a * cover, a * cover);
}

// linear -> sRGB. The inverse of what the sampler already did for us; needed
// because the filters below are defined on the encoded values, not on light.
fn frus_linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lower = c * 12.92;
    let higher = 1.055 * pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(higher, lower, c <= vec3<f32>(0.0031308));
}

fn frus_srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lower = c / 12.92;
    let higher = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(higher, lower, c <= vec3<f32>(0.04045));
}

const FRUS_EDGE: f32 = 1e-4;

// One channel of a separable blend, on **straight** values.
fn frus_separable(sc: f32, dc: f32, mode: u32) -> f32 {
    if (mode == 13u) {                      // Multiply
        return sc * dc;
    } else if (mode == 14u) {               // Screen
        return sc + dc - sc * dc;
    } else if (mode == 15u) {               // Overlay: Hard-light with the roles swapped
        if (dc <= 0.5) {
            return 2.0 * sc * dc;
        }
        return 1.0 - 2.0 * (1.0 - sc) * (1.0 - dc);
    } else if (mode == 16u) {               // Darken
        return min(sc, dc);
    } else if (mode == 17u) {               // Lighten
        return max(sc, dc);
    } else if (mode == 19u) {               // ColorDodge
        if (dc <= FRUS_EDGE) {
            return 0.0;
        }
        if (sc >= 1.0 - FRUS_EDGE) {
            return 1.0;
        }
        return min(1.0, dc / (1.0 - sc));
    } else if (mode == 20u) {               // ColorBurn
        if (dc >= 1.0 - FRUS_EDGE) {
            return 1.0;
        }
        if (sc <= FRUS_EDGE) {
            return 0.0;
        }
        return 1.0 - min(1.0, (1.0 - dc) / sc);
    } else if (mode == 21u) {               // HardLight: Overlay with the roles swapped
        if (sc <= 0.5) {
            return 2.0 * sc * dc;
        }
        return 1.0 - 2.0 * (1.0 - sc) * (1.0 - dc);
    } else if (mode == 22u) {               // SoftLight
        if (sc <= 0.5) {
            return dc - (1.0 - 2.0 * sc) * dc * (1.0 - dc);
        }
        var d = sqrt(dc);
        if (dc <= 0.25) {
            d = ((16.0 * dc - 12.0) * dc + 4.0) * dc;
        }
        return dc + (2.0 * sc - 1.0) * (d - dc);
    } else if (mode == 23u) {               // Difference
        return abs(sc - dc);
    }
    return sc + dc - 2.0 * sc * dc;         // 24 Exclusion
}

// The non-separable modes' helpers: luminosity, saturation, and setting each while
// keeping the colour in gamut.
fn frus_lum(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.3, 0.59, 0.11));
}

fn frus_clip_color(c: vec3<f32>) -> vec3<f32> {
    let l = frus_lum(c);
    let n = min(min(c.r, c.g), c.b);
    let x = max(max(c.r, c.g), c.b);
    var o = c;
    if (n < 0.0) {
        o = vec3<f32>(l) + (o - vec3<f32>(l)) * l / max(l - n, 1e-6);
    }
    if (x > 1.0) {
        o = vec3<f32>(l) + (o - vec3<f32>(l)) * (1.0 - l) / max(x - l, 1e-6);
    }
    return o;
}

fn frus_set_lum(c: vec3<f32>, l: f32) -> vec3<f32> {
    return frus_clip_color(c + vec3<f32>(l - frus_lum(c)));
}

fn frus_sat(c: vec3<f32>) -> f32 {
    return max(max(c.r, c.g), c.b) - min(min(c.r, c.g), c.b);
}

fn frus_set_sat(c: vec3<f32>, s: f32) -> vec3<f32> {
    let lo = min(min(c.r, c.g), c.b);
    let hi = max(max(c.r, c.g), c.b);
    if (hi > lo) {
        return (c - vec3<f32>(lo)) * s / (hi - lo);
    }
    return vec3<f32>(0.0);
}

// One of the four non-separable modes, on **straight** colours.
fn frus_non_separable(sc: vec3<f32>, dc: vec3<f32>, mode: u32) -> vec3<f32> {
    if (mode == 25u) {                      // Hue
        return frus_set_lum(frus_set_sat(sc, frus_sat(dc)), frus_lum(dc));
    } else if (mode == 26u) {               // Saturation
        return frus_set_lum(frus_set_sat(dc, frus_sat(sc)), frus_lum(dc));
    } else if (mode == 27u) {               // Color
        return frus_set_lum(sc, frus_lum(dc));
    }
    return frus_set_lum(dc, frus_lum(sc));            // 28 Luminosity
}

// `src` over `dst` under `mode`, both **premultiplied**. The Porter-Duff modes are
// linear in the premultiplied values, which is why they are one line each; the
// separable modes are not, so they are unpremultiplied first and the standard
// composition is spelled out.
fn frus_blend(src: vec4<f32>, dst: vec4<f32>, mode: u32) -> vec4<f32> {
    let sa = src.a;
    let da = dst.a;
    if (mode == 0u) { return src; }                                  // Src
    if (mode == 1u) { return dst; }                                  // Dst
    if (mode == 2u) { return src + dst * (1.0 - sa); }               // SrcOver
    if (mode == 3u) { return dst + src * (1.0 - da); }               // DstOver
    if (mode == 4u) { return src * da; }                             // SrcIn
    if (mode == 5u) { return dst * sa; }                             // DstIn
    if (mode == 6u) { return src * (1.0 - da); }                     // SrcOut
    if (mode == 7u) { return dst * (1.0 - sa); }                     // DstOut
    if (mode == 8u) { return src * da + dst * (1.0 - sa); }          // SrcAtop
    if (mode == 9u) { return dst * sa + src * (1.0 - da); }          // DstAtop
    if (mode == 10u) {                                               // Xor
        return src * (1.0 - da) + dst * (1.0 - sa);
    }
    if (mode == 11u) {                                               // Plus
        return min(src + dst, vec4<f32>(1.0));
    }
    if (mode == 12u) {                                               // Modulate
        return src * dst;
    }
    if (mode == 18u) {                                               // Clear
        return vec4<f32>(0.0);
    }
    // The separable set and the four that are not: unpremultiply, blend, recompose.
    let sc = select(src.rgb / sa, vec3<f32>(0.0), sa <= 0.0);
    let dc = select(dst.rgb / da, vec3<f32>(0.0), da <= 0.0);
    var b: vec3<f32>;
    if (mode >= 25u) {
        b = frus_non_separable(sc, dc, mode);
    } else {
        b = vec3<f32>(
            frus_separable(sc.r, dc.r, mode),
            frus_separable(sc.g, dc.g, mode),
            frus_separable(sc.b, dc.b, mode),
        );
    }
    let rgb = (1.0 - da) * src.rgb + (1.0 - sa) * dst.rgb + sa * da * b;
    return vec4<f32>(rgb, sa + da - sa * da);
}

