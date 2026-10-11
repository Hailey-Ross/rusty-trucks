#import bevy_sprite::mesh2d_vertex_output::VertexOutput
struct ColorTransform { multiply: vec4<f32>, add: vec4<f32>, mode: vec4<f32> }
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> color: ColorTransform;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var atlas: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var atlas_sampler: sampler;

fn to_gamma(c: vec3<f32>) -> vec3<f32> {
    return select(c * 12.92, 1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055, c > vec3(0.0031308));
}
fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(c / 12.92, pow((c + 0.055) / 1.055, vec3(2.4)), c > vec3(0.04045));
}

// mode.x = 1 (the retail menus): APT colour transforms and solid colours are display (gamma) values, like the
// console's front end; the atlas is sampled as sRGB (linear here) and the target encodes sRGB, so the transform
// is applied in gamma space and the result handed back as linear. A texture with an identity transform comes out
// unchanged. Both the menus and the gameplay HUD use it (one APT player in retail); mode.x = 0 keeps the old
// linear path for any other user.
@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let t = textureSample(atlas, atlas_sampler, in.uv);
    if color.mode.x < 0.5 {
        return clamp(t * color.multiply + color.add, vec4(0.0), vec4(1.0));
    }
    let g = clamp(vec4(to_gamma(t.rgb), t.a) * color.multiply + color.add, vec4(0.0), vec4(1.0));
    return vec4(to_linear(g.rgb), g.a);
}
