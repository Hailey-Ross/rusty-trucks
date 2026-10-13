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

// The layer target holds display (gamma) values and blends them there, like the console's front end: retail's APT
// draw state is straight SRC_ALPHA / INV_SRC_ALPHA (`sub_827E4AF0`, bound by `sub_825D4490`) on a display-value
// image (research menus-m6 "Retail blend state", medium-high confidence: the 2D pass's format is inferred). Blending
// in linear made the soft low-alpha glow layers of a selected menu row far too bright (play test, 2026-10-12). The
// composite (`hud_composite.wgsl`) converts the layer to linear once.
// mode.x = 1 (the retail menus and the gameplay HUD, one APT player in retail): the colour transform applies to the
// display value of the texel. mode.x = 0 keeps the linear transform for any other user, stored as display values.
@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let t = textureSample(atlas, atlas_sampler, in.uv);
    if color.mode.x < 0.5 {
        let l = clamp(t * color.multiply + color.add, vec4(0.0), vec4(1.0));
        return vec4(to_gamma(l.rgb), l.a);
    }
    return clamp(vec4(to_gamma(t.rgb), t.a) * color.multiply + color.add, vec4(0.0), vec4(1.0));
}
