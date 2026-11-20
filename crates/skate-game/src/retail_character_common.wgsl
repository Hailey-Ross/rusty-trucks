#define_import_path skate_character_lighting
#import bevy_pbr::mesh_view_bindings as frame
#import bevy_pbr::shadows::fetch_directional_shadow
struct CharacterParams {
    light: vec4<f32>, tint: vec4<f32>, options: vec4<f32>,
    rows: array<vec4<f32>,9>, sh: array<vec4<f32>,9>,
    // Hair highlight tint (cac_hair i_modulateColor2: a CAS color pick's recipe B); w = 1 when set.
    highlight_tint: vec4<f32>,
}

// Both original and modular stock meshes use the authored derivative basis.
// Modular GLBs intentionally omit tangents, so StandardMaterial's tangent-only
// normal mapping cannot supply this detail after an outfit/model swap.
fn character_normal(position: vec4<f32>, normal: vec3<f32>, uv: vec2<f32>, sample: vec2<f32>) -> vec3<f32> {
    let n=normalize(normal);
    let rpos=position.xyz-frame::view.world_position;
    let dp1=dpdx(rpos); let dp2=dpdy(rpos);
    let du1=dpdx(uv); let du2=dpdy(uv);
    var tt=cross(dp2,n)*du1.x+cross(n,dp1)*du2.x;
    var bb=cross(dp2,n)*du1.y+cross(n,dp1)*du2.y;
    tt *= -inverseSqrt(max(dot(tt,tt),1e-12));
    bb *= inverseSqrt(max(dot(bb,bb),1e-12));
    let nm=sample*2.0-1.0;
    return normalize(nm.x*tt+nm.y*bb+n*sqrt(saturate(1.0-dot(nm,nm))));
}
fn character_albedo(linear: vec3<f32>) -> vec3<f32> {
    let gamma=select(linear*12.92,1.055*pow(max(linear,vec3<f32>(0.0)),vec3<f32>(1.0/2.4))-0.055,linear>vec3<f32>(0.0031308));
    return gamma*gamma;
}

fn shade_character(p: CharacterParams, vn: vec3<f32>, position: vec4<f32>, d: vec3<f32>, lin: vec3<f32>, smask: f32, alpha: f32) -> vec4<f32> {
    let vd = -normalize(position.xyz-frame::view.world_position);
    if p.options.w>0.0 {
        // Native hair response: lin x (light x MOD1 + highlight x MOD2) (cac_hair_defaultPS, doc 43 "Color rows").
        let ndl=saturate(dot(vn,p.light.xyz));
        let fres=pow(1.0-saturate(dot(vn,vd)),max(p.rows[2].w,1.0));
        let lit=p.rows[1].rgb*(saturate(ndl*0.75+0.25)+p.rows[5].w*0.25);
        let highlight=p.rows[6].rgb*fres*saturate(ndl*1.75+0.25);
        let hd=select(d,lin*p.highlight_tint.rgb,p.highlight_tint.w>0.0);
        if alpha<p.options.z { discard; }
        return vec4<f32>((d*lit+hd*highlight)*p.light.w,alpha*p.tint.a);
    }
    let s=p.rows[7].y;
    let v=vn*s;
    let irr=saturate(p.sh[0].rgb+v.x*p.sh[1].rgb+v.y*p.sh[2].rgb+v.z*p.sh[3].rgb
        +v.x*v.z*p.sh[4].rgb+v.z*v.y*p.sh[5].rgb+v.y*v.x*p.sh[6].rgb
        +(3.0*v.z*v.z-1.0)*p.sh[7].rgb+(v.x*v.x-v.y*v.y)*p.sh[8].rgb);
    let ndl=dot(vn,p.light.xyz);
    let view_z=(frame::view.view_from_world*position).z;
    var shadow=1.0;
    for (var light_id=0u; light_id<frame::lights.n_directional_lights; light_id+=1u) {
        // All-caster visibility light does not affect lightmapped world diffuse.
        if (frame::lights.directional_lights[light_id].flags & 13u)==1u {
            shadow=fetch_directional_shadow(light_id,position,vn,view_z);
            break;
        }
    }
    var lit=p.rows[1].rgb*saturate(ndl)*shadow+irr*p.rows[5].w;
    let fb=1.0-saturate(dot(vn,vd));
    let kfres=pow(max(fb,1e-6),p.rows[1].w);
    let rfres=pow(max(fb,1e-6),p.rows[0].z);
    let rd=normalize(p.light.xyz*vec3<f32>(-1.,0.2,-1.)-vd);
    lit+=saturate(rfres*saturate(dot(vn,rd)))*p.rows[6].rgb;
    let kr=p.light.xyz-2.0*ndl*vn;
    let ks=pow(saturate(dot(vd,-kr)),p.rows[2].w)*select(0.0,1.0,ndl>=0.0)*shadow;
    let rr=rd-2.0*dot(vn,rd)*vn;
    let rs=pow(saturate(dot(vd,-rr)),p.rows[3].w)*rfres;
    let spec=saturate((ks*p.rows[2].rgb*kfres+rs*p.rows[3].rgb)*smask*smask);
    if alpha<p.options.z { discard; }
    return vec4<f32>((d*lit+spec)*p.rows[0].y*p.light.w,alpha*p.tint.a);
}
