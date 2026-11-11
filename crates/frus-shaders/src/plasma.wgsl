// A plasma: three sine waves, mixed between two colours the application sets.
struct Uniforms {
    deep: vec4<f32>,
    bright: vec4<f32>,
    scale: f32,
};

fn shade(frag: Fragment) -> vec4<f32> {
    let p = frag.uv * uniforms.scale;
    let t = frag.time;
    var v = sin(p.x + t);
    v = v + sin(p.y * 1.3 - t * 0.7);
    v = v + sin(length(p - vec2<f32>(uniforms.scale * 0.5)) * 1.7 + t * 1.3);
    let k = 0.5 + 0.5 * sin(v * 1.6);
    return vec4<f32>(mix(uniforms.deep.rgb, uniforms.bright.rgb, k), 1.0);
}
