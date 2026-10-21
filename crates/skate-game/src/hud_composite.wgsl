#import bevy_ui::ui_vertex_output::UiVertexOutput
@group(1) @binding(0) var hud: texture_2d<f32>;
@group(1) @binding(1) var hud_sampler: sampler;
// 1 with `SKATE_HUD_RECT`, when the node may cover only part of the screen: each of its pixels
// loads the target's pixel under it (scoring_hud.rs `render`).
@group(1) @binding(2) var<uniform> load: u32;
@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    if load != 0u {
        let last = vec2<i32>(textureDimensions(hud)) - 1;
        return textureLoad(hud, clamp(vec2<i32>(in.position.xy), vec2<i32>(0), last), 0);
    }
    return textureSample(hud, hud_sampler, in.uv);
}
