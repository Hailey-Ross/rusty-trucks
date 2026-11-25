#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
// NIS depth of field (nis_dof.rs; retail bloom_dof_dof_downsamplePS + bloom_dof_tap9dofPS + the uber blend).
@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;
@group(0) @binding(2) var depth: texture_depth_2d;
// [0] focalFar, farBlur, focalNear, nearBlur; [1] maxCoC, min radius, pixels per unit, camera near.
@group(0) @binding(3) var<uniform> p: array<vec4<f32>, 3>;

fn view_depth(uv: vec2<f32>) -> f32 {
    let size = vec2<f32>(textureDimensions(depth));
    let d = textureLoad(depth, vec2<i32>(clamp(uv * size, vec2<f32>(0.0), size - 1.0)), 0);
    return select(1.0e9, p[1].w / d, d > 0.0);
}

// The far plane from focalNear on, else the near plane: saturate((plane.y - z) * plane.x).
fn coc(uv: vec2<f32>) -> f32 {
    let z = view_depth(uv);
    if (z >= p[0].z) {
        return saturate((p[0].x - z) * (-1.0 / max(p[0].y - p[0].x, 1.0e-4)));
    }
    return saturate((p[0].z - z) * (1.0 / max(p[0].z - p[0].w, 1.0e-4)));
}

@fragment
fn fragment(i: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let sharp = textureSample(source, source_sampler, i.uv);
    let texel = 1.0 / vec2<f32>(textureDimensions(source));
    let radius = max(coc(i.uv) * p[1].x, p[1].y) * p[1].z;
    // Nine taps on a 3 x 3 grid, each weighted by its own CoC.
    var sum = vec3<f32>(0.0);
    var weight = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let uv = i.uv + vec2<f32>(f32(x), f32(y)) * radius * texel;
            let w = coc(uv) + 1.0e-3;
            sum += textureSample(source, source_sampler, uv).rgb * w;
            weight += w;
        }
    }
    let blurred = sum / weight;
    let amount = saturate(weight / 9.0 - 1.0e-3);
    return vec4<f32>(mix(sharp.rgb, blurred, amount), sharp.a);
}
