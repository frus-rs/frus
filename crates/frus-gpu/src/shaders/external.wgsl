// The conversion pass of external textures: documented in `external.rs`.

struct Params {
    mode: u32,
    turns: u32,
    mirrored: u32,
    _pad: u32,
    row_r: vec4<f32>,
    row_g: vec4<f32>,
    row_b: vec4<f32>,
    offset: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> params: Params;
@group(0) @binding(1)
var plane0: texture_2d<f32>;
@group(0) @binding(2)
var plane1: texture_2d<f32>;
@group(0) @binding(3)
var plane2: texture_2d<f32>;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let x = f32((index << 1u) & 2u);
    let y = f32(index & 2u);
    return vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

fn chroma(plane: texture_2d<f32>, p: vec2<f32>) -> vec4<f32> {
    let size = vec2<i32>(textureDimensions(plane));
    let c = p * 0.5 - vec2<f32>(0.5);
    let base = floor(c);
    let f = c - base;
    let i0 = clamp(vec2<i32>(base), vec2<i32>(0), size - vec2<i32>(1));
    let i1 = clamp(vec2<i32>(base) + vec2<i32>(1), vec2<i32>(0), size - vec2<i32>(1));
    let a = textureLoad(plane, vec2<i32>(i0.x, i0.y), 0);
    let b = textureLoad(plane, vec2<i32>(i1.x, i0.y), 0);
    let c0 = textureLoad(plane, vec2<i32>(i0.x, i1.y), 0);
    let d = textureLoad(plane, vec2<i32>(i1.x, i1.y), 0);
    return mix(mix(a, b, f.x), mix(c0, d, f.x), f.y);
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lower = c / 12.92;
    let higher = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(higher, lower, c <= vec3<f32>(0.04045));
}

fn source(at_target: vec2<f32>) -> vec2<f32> {
    let size = vec2<f32>(textureDimensions(plane0));
    var p = at_target;
    if params.turns == 1u {
        p = vec2<f32>(at_target.y, size.y - at_target.x);
    } else if params.turns == 2u {
        p = size - at_target;
    } else if params.turns == 3u {
        p = vec2<f32>(size.x - at_target.y, at_target.x);
    }
    if params.mirrored != 0u {
        p.x = size.x - p.x;
    }
    return p;
}

@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let at = source(pos.xy);
    let pixel = vec2<i32>(floor(at));
    if params.mode == 0u {
        let s = textureLoad(plane0, pixel, 0);
        return vec4<f32>(s.b, s.g, s.r, s.a);
    }
    if params.mode == 3u {
        return textureLoad(plane0, pixel, 0);
    }
    let y = textureLoad(plane0, pixel, 0).r;
    var uv: vec2<f32>;
    if params.mode == 1u {
        uv = chroma(plane1, at).rg;
    } else {
        uv = vec2<f32>(chroma(plane1, at).r, chroma(plane2, at).r);
    }
    let yuv = vec4<f32>(y, uv, 1.0) - params.offset;
    let rgb = clamp(
        vec3<f32>(dot(params.row_r, yuv), dot(params.row_g, yuv), dot(params.row_b, yuv)),
        vec3<f32>(0.0),
        vec3<f32>(1.0),
    );
    return vec4<f32>(srgb_to_linear(rgb), 1.0);
}
