// The GPU pass's triangle: turned by `angle.x`, kept round by the aspect in `angle.y`.
struct Angle {
    v: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> angle: Angle;

struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec3<f32>,
};

@vertex
fn vs(@builtin(vertex_index) i: u32) -> Out {
    let a = angle.v.x + f32(i) * 2.0943951;
    var out: Out;
    out.position = vec4<f32>(0.8 * cos(a) / angle.v.y, 0.8 * sin(a), 0.0, 1.0);
    var colors = array<vec3<f32>, 3>(
        vec3<f32>(1.0, 0.2, 0.2),
        vec3<f32>(0.2, 1.0, 0.3),
        vec3<f32>(0.2, 0.4, 1.0),
    );
    out.color = colors[i];
    return out;
}

@fragment
fn fs(in: Out) -> @location(0) vec4<f32> {
    // Linear light, premultiplied and opaque: what a GPU pass draws.
    return vec4<f32>(in.color, 1.0);
}
