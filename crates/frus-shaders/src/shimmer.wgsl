// A bright band running across what it masks, over a base colour.
struct Uniforms {
    base: vec4<f32>,
};

fn shade(frag: Fragment) -> vec4<f32> {
    let head = fract(frag.time * 0.4) * 1.6 - 0.3;
    let band = 1.0 - smoothstep(0.0, 0.15, abs(frag.uv.x - head));
    return vec4<f32>(mix(uniforms.base.rgb, vec3<f32>(1.0), band), 1.0);
}
