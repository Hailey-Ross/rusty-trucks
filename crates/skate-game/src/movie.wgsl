#import bevy_ui::ui_vertex_output::UiVertexOutput
// A movie frame from three single-channel planes (YUV 4:2:0, BT.601 limited range as VP6 stores it), shown on the UI
// (docs/hails-additions/47-movie-player.md).
@group(1) @binding(0) var luma: texture_2d<f32>;
@group(1) @binding(1) var luma_sampler: sampler;
@group(1) @binding(2) var chroma_u: texture_2d<f32>;
@group(1) @binding(3) var chroma_u_sampler: sampler;
@group(1) @binding(4) var chroma_v: texture_2d<f32>;
@group(1) @binding(5) var chroma_v_sampler: sampler;
fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(c / 12.92, pow((c + 0.055) / 1.055, vec3(2.4)), c > vec3(0.04045));
}
@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    let y = (textureSample(luma, luma_sampler, in.uv).r - 16.0 / 255.0) * (255.0 / 219.0);
    let u = (textureSample(chroma_u, chroma_u_sampler, in.uv).r - 128.0 / 255.0) * (255.0 / 224.0);
    let v = (textureSample(chroma_v, chroma_v_sampler, in.uv).r - 128.0 / 255.0) * (255.0 / 224.0);
    let rgb = vec3(y + 1.402 * v, y - 0.344136 * u - 0.714136 * v, y + 1.772 * u);
    return vec4(to_linear(clamp(rgb, vec3(0.0), vec3(1.0))), 1.0);
}
