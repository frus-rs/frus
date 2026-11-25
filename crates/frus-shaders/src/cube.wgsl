// The 3D page's cube: 36 vertices made here from their index (no vertex buffer), turned by
// the time in `params.x`, seen through a perspective kept square by the aspect in `params.y`,
// lit by one light. Faces are not culled: what hides the back faces is the depth test.
struct Params {
    v: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> params: Params;

struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec3<f32>,
};

fn rotate_y(p: vec3<f32>, a: f32) -> vec3<f32> {
    return vec3<f32>(p.x * cos(a) + p.z * sin(a), p.y, -p.x * sin(a) + p.z * cos(a));
}

fn rotate_x(p: vec3<f32>, a: f32) -> vec3<f32> {
    return vec3<f32>(p.x, p.y * cos(a) - p.z * sin(a), p.y * sin(a) + p.z * cos(a));
}

@vertex
fn vs(@builtin(vertex_index) i: u32) -> Out {
    var normals = array<vec3<f32>, 6>(
        vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(-1.0, 0.0, 0.0),
        vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(0.0, -1.0, 0.0),
        vec3<f32>(0.0, 0.0, 1.0), vec3<f32>(0.0, 0.0, -1.0),
    );
    var colors = array<vec3<f32>, 6>(
        vec3<f32>(0.90, 0.25, 0.20), vec3<f32>(0.20, 0.60, 0.95),
        vec3<f32>(0.30, 0.85, 0.35), vec3<f32>(0.95, 0.80, 0.20),
        vec3<f32>(0.75, 0.35, 0.90), vec3<f32>(0.95, 0.55, 0.15),
    );
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let face = i / 6u;
    let n = normals[face];
    // Two directions across the face.
    var u = vec3<f32>(0.0, 1.0, 0.0);
    if abs(n.y) > 0.5 {
        u = vec3<f32>(0.0, 0.0, 1.0);
    }
    let v = cross(n, u);
    let c = corners[i % 6u];
    let t = params.v.x;
    let local = 0.5 * (n + u * c.x + v * c.y);
    let world = rotate_x(rotate_y(local, t), t * 0.6);
    let normal = rotate_x(rotate_y(n, t), t * 0.6);

    // The camera 2.4 units back, a 45 degree view, depth from 0.1 to 10.
    let z = world.z - 2.4;
    let f = 2.4142135;
    let near = 0.1;
    let far = 10.0;
    var out: Out;
    out.position = vec4<f32>(
        world.x * f / params.v.y,
        world.y * f,
        z * far / (near - far) + near * far / (near - far),
        -z,
    );
    let light = normalize(vec3<f32>(0.4, 0.7, 0.6));
    let lit = 0.25 + 0.75 * max(dot(normal, light), 0.0);
    out.color = colors[face] * lit;
    return out;
}

@fragment
fn fs(in: Out) -> @location(0) vec4<f32> {
    // Linear light, premultiplied and opaque: what a GPU pass draws.
    return vec4<f32>(in.color, 1.0);
}
