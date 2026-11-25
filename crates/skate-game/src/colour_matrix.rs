//! Retail post FX color matrix records from the stock collections (class `post_fx`; research b100): the base
//! `colour_matrix` (identity) and `colour_matrix_zombiemode` (the zombie cheat's tint, selected by `827F4D28` /
//! `827F5450` while the zombie query is true). The matrices are built by `skate_core::colour_matrix`.

use bevy::prelude::*;
use skate_core::colour_matrix::{ColourBand, ColourMatrixRecord};
use skate_data::collections::Collections;

/// What the retail tone pass applies (`postfx_visualfxPS`, b101): band rows, depth weights, replay tint: docs/hails-additions/35-world-shading-parity.md#tone-pass-inputs
#[derive(Resource, Clone, Copy, Debug, PartialEq, bevy::render::extract_resource::ExtractResource)]
pub(crate) struct ColourGrade {
    /// 0..2 near, 3..5 far, 6 depth weights, 7 camera near, 8 replay tint, 9 `f_bloomColor` (rgb, w).
    pub rows: [Vec4; 10],
}

impl ColourGrade {
    /// The matrices and depth weights of a record (identity record: the picture is unchanged).
    pub fn of(record: &ColourMatrixRecord, camera_near: f32) -> Self {
        let near = skate_core::colour_matrix::build(&record.near);
        let far = skate_core::colour_matrix::build(&record.far);
        let d = (record.distance_far - record.distance_near).max(1e-3);
        let row = |r: [f32; 4]| Vec4::from_array(r);
        Self {
            rows: [
                row(near[0]),
                row(near[1]),
                row(near[2]),
                row(far[0]),
                row(far[1]),
                row(far[2]),
                Vec4::new(-1.0 / d, 1.0 / d, record.distance_far / d, -record.distance_near / d),
                Vec4::new(camera_near, 0.0, 0.0, 0.0),
                Vec4::ZERO,
                Vec4::ZERO,
            ],
        }
    }
}

impl Default for ColourGrade {
    fn default() -> Self {
        Self::of(&ColourMatrixRecord::default(), 0.1)
    }
}

/// The stock records (loaded once; a missing collection keeps the identity).
#[derive(Resource, Default)]
pub(crate) struct ColourRecords {
    pub base: ColourMatrixRecord,
    pub zombie: Option<ColourMatrixRecord>,
    /// `f_bloomColor.rgb` (827EF1F0: the `bloom` record's color + 1.0); None without the record (no bloom step).
    pub bloom_colour: Option<Vec3>,
}

pub(crate) fn load_records(config: Option<Res<crate::config::Config>>, mut records: ResMut<ColourRecords>) {
    let Some(config) = config else { return };
    match Collections::load(&config.asset_root) {
        Ok(c) => {
            records.base = load(&c, BASE).unwrap_or_default();
            records.zombie = load(&c, ZOMBIE).map_err(|e| warn!("COLOUR_MATRIX {ZOMBIE}: {e}")).ok();
            records.bloom_colour = bloom_colour(&c).map(|v| Vec3::from_array(v) + 1.0)
                .map_err(|e| warn!("COLOUR_MATRIX {BLOOM}: {e}")).ok();
        }
        Err(e) => warn!("COLOUR_MATRIX: stock collections: {e}"),
    }
}

/// The active record (`827F4D28` sets the zombie bit while the zombie query is true; `827F5450` then builds the
/// zombie matrices, a full replace with no fade) and the gameplay camera's near plane.
pub(crate) fn update_grade(
    records: Res<ColourRecords>,
    settings: Option<Res<crate::living_world::LivingWorldSettings>>,
    cameras: Query<&Projection, With<crate::camera::GameplayCamera>>,
    replay: Option<Res<crate::replay::Replay>>,
    mut grade: ResMut<ColourGrade>,
) {
    let zombie = settings.is_some_and(|s| s.zombie);
    let world = if zombie { records.zombie.as_ref().unwrap_or(&records.base) } else { &records.base };
    // During a replay the editor's look lanes replace their terms of the world record (b380 / b408: retail's override
    // copy of the color block, built into the same matrices).
    let look = replay.as_ref().and_then(|r| r.look(world));
    let mut record = *look.as_ref().map_or(world, |l| &l.record);
    // A reel clip's NIS take writes its color lanes into the environment block while it plays (b1064), and the
    // photo shot picker's edits add to them on the held photo NIS (b1063: the ScenePhotoCustomizer values).
    if let Some(colour) = replay.as_ref().filter(|r| r.active).and_then(|r| r.reel_shot.as_ref()).map(|s| s.colour) {
        record = with_take_colour(&record, &colour);
    }
    if let Some(edits) = replay.as_ref().filter(|r| r.reel_still).and_then(|r| r.photo_look) {
        record = with_photo_edits(&record, edits);
    }
    let record = &record;
    let near = cameras
        .iter()
        .find_map(|p| match p {
            Projection::Perspective(p) => Some(p.near),
            _ => None,
        })
        .unwrap_or(0.1);
    let mut next = ColourGrade::of(record, near);
    // The replay's tint (b413, `827FE770`): one constant color alpha-blended over the whole graded frame, under the
    // HUD and menus; drawn as the tone pass's last step so the exported video carries it too.
    if let Some(look) = look.as_ref() {
        next.rows[8] = Vec4::from_array(look.tint);
    }
    // The screen fade block's +0x18 (the reel's BloomToWhite transition) turns the visualFX pass on above 0.001
    // (827F4D28) and lifts f_bloomColor.w = clamp01(threshold - (threshold - 2 x)) = clamp01(2 x) (827F5450 ->
    // 827EF1F0): the frame goes toward its luma in the bloom color.
    let x = replay.as_ref().map_or(0.0, |r| r.reel_bloom());
    if let Some(colour) = records.bloom_colour.filter(|_| x > 0.001) {
        next.rows[9] = colour.extend((2.0 * x).clamp(0.0, 1.0));
    }
    if *grade != next {
        *grade = next;
    }
}

/// An NIS take's color lanes over a record (IceEnv_SetBrightness 82678F88 and its contrast / saturation twins write
/// the environment block from the take when the lane's override is on, research b1064): brightness is each band's
/// add, contrast and midpoint replace the band's, saturation (retail neutral 0) is the near value for both bands
/// (b380) as 1 + s in our band [inferred: our identity band saturation is 1].
pub(crate) fn with_take_colour(record: &ColourMatrixRecord, c: &skate_data::ice_camera::TakeColour) -> ColourMatrixRecord {
    let mut r = *record;
    if let Some([near, far]) = c.brightness {
        r.near.add = [near; 3];
        r.far.add = [far; 3];
    }
    if let Some(([near, far], [mid_near, mid_far])) = c.contrast {
        (r.near.contrast, r.far.contrast) = (near, far);
        (r.near.contrast_midpoint, r.far.contrast_midpoint) = (mid_near, mid_far);
    }
    if let Some(s) = c.saturation {
        r.near.saturation = 1.0 + s;
        r.far.saturation = 1.0 + s;
    }
    r
}

/// The photo picker's [brightness, contrast, saturation] over a record (research b1063): IceEnv brightness (82678F88)
/// adds b to both bands' add rgb, contrast (82679D70) adds c to both bands' contrast, saturation (8267A250) adds s to
/// the near value the renderer uses for both bands (b380), all on top of the photo take's own lanes
/// ([`with_take_colour`]).
pub(crate) fn with_photo_edits(record: &ColourMatrixRecord, [b, c, s]: [f32; 3]) -> ColourMatrixRecord {
    let mut r = *record;
    let saturation = r.near.saturation + s;
    for band in [&mut r.near, &mut r.far] {
        band.add = band.add.map(|a| a + b);
        band.contrast += c;
        band.saturation = saturation;
    }
    r
}

pub(crate) const CLASS: &str = "post_fx";
pub(crate) const BASE: &str = "colour_matrix";
pub(crate) const ZOMBIE: &str = "colour_matrix_zombiemode";
/// The `post_fx` record bound as the bloom (827EB510) and its color field (b679: (0, 0.5, 1) on the disc).
pub(crate) const BLOOM: &str = "bloom";
pub(crate) const BLOOM_COLOUR: &str = "Hash_B967801EE8EFFEB8";

fn vec3(c: &Collections, record: &str, name: &str) -> Result<[f32; 3], String> {
    let w = c.words::<3>(CLASS, record, name)?;
    let v = [f32::from_bits(w[0]), f32::from_bits(w[1]), f32::from_bits(w[2])];
    if v.iter().all(|x| x.is_finite()) { Ok(v) } else { Err(format!("Non-finite {CLASS}/{record}/{name}")) }
}

/// The bloom color (a Math::Vector3 stored padded to four words).
fn bloom_colour(c: &Collections) -> Result<[f32; 3], String> {
    let w = c.words::<4>(CLASS, BLOOM, BLOOM_COLOUR)?;
    let v = [f32::from_bits(w[0]), f32::from_bits(w[1]), f32::from_bits(w[2])];
    if v.iter().all(|x| x.is_finite()) { Ok(v) } else { Err(format!("Non-finite {CLASS}/{BLOOM}/{BLOOM_COLOUR}")) }
}

fn band(c: &Collections, record: &str, side: &str) -> Result<ColourBand, String> {
    let f = |n: &str| c.float(CLASS, record, &format!("{side}_{n}"));
    Ok(ColourBand {
        multiply: vec3(c, record, &format!("{side}_multiply"))?,
        add: vec3(c, record, &format!("{side}_add"))?,
        contrast: f("contrast")?,
        contrast_midpoint: f("contrast_midpoint")?,
        saturation: f("saturation")?,
    })
}

/// One record (parents included).
pub(crate) fn load(c: &Collections, record: &str) -> Result<ColourMatrixRecord, String> {
    Ok(ColourMatrixRecord {
        near: band(c, record, "near")?,
        far: band(c, record, "far")?,
        distance_near: c.float(CLASS, record, "distance_near")?,
        distance_far: c.float(CLASS, record, "distance_far")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn photo_edits_offset_both_bands_and_saturation_follows_the_near_band() {
        let mut base = ColourMatrixRecord::default();
        base.far.saturation = 0.5;
        let r = with_photo_edits(&base, [0.1, -0.25, 0.75]);
        assert_eq!((r.near.add, r.far.add), ([0.1; 3], [0.1; 3]));
        assert_eq!((r.near.contrast, r.far.contrast), (0.75, 0.75));
        assert_eq!((r.near.saturation, r.far.saturation), (1.75, 1.75), "the far band takes the near value");
        assert_eq!(with_photo_edits(&ColourMatrixRecord::default(), [0.0; 3]), ColourMatrixRecord::default());
    }

    #[test]
    fn a_take_colour_writes_only_its_switched_on_lanes() {
        use skate_data::ice_camera::TakeColour;
        let base = ColourMatrixRecord::default();
        assert_eq!(with_take_colour(&base, &TakeColour::default()), base, "all lanes off: the world grade");
        let take = TakeColour { brightness: Some([0.1, -0.1]), contrast: Some(([1.2, 0.8], [0.4, 0.6])), saturation: Some(-0.5) };
        let r = with_take_colour(&base, &take);
        assert_eq!((r.near.add, r.far.add), ([0.1; 3], [-0.1; 3]));
        assert_eq!((r.near.contrast, r.far.contrast, r.near.contrast_midpoint, r.far.contrast_midpoint), (1.2, 0.8, 0.4, 0.6));
        assert_eq!((r.near.saturation, r.far.saturation), (0.5, 0.5));
        // A neutral photo take (the disc's 0 / 1 / 0) leaves the grade as it was, so the picker's edits are the only change.
        let neutral = TakeColour { brightness: Some([0.0; 2]), contrast: Some(([1.0; 2], [0.5; 2])), saturation: Some(0.0) };
        assert_eq!(with_photo_edits(&with_take_colour(&base, &neutral), [0.05, 0.25, -0.5]).near.saturation, 0.5);
    }

    /// The grade the tone pass gets: the near / far weights are linear in view depth between the record's distances
    /// (full near up to 50 m, full far from 100 m, summing to 1); the base record leaves every color unchanged.
    #[test]
    fn the_grade_mixes_near_and_far_linearly_in_depth() {
        let g = ColourGrade::of(&ColourMatrixRecord::default(), 0.25);
        let w = |z: f32| ((z * g.rows[6].x + g.rows[6].z).clamp(0.0, 1.0), (z * g.rows[6].y + g.rows[6].w).clamp(0.0, 1.0));
        assert_eq!(w(10.0), (1.0, 0.0));
        assert_eq!(w(75.0), (0.5, 0.5));
        assert_eq!(w(500.0), (0.0, 1.0));
        assert_eq!(g.rows[7].x, 0.25);
        assert_eq!(g.rows[8], Vec4::ZERO, "no replay tint");
        let apply = |base: usize, c: Vec3| Vec3::new(g.rows[base].dot(c.extend(1.0)), g.rows[base + 1].dot(c.extend(1.0)), g.rows[base + 2].dot(c.extend(1.0)));
        let c = Vec3::new(0.3, 0.6, 0.9);
        assert!((apply(0, c) - c).length() < 1e-6 && (apply(3, c) - c).length() < 1e-6, "identity");
    }

    /// Data-gated: the base record is the identity and the zombie record holds the values b100 read.
    #[test]
    fn stock_colour_matrix_records_load() {
        let Some(root) = std::env::var_os("SKATE3_ASSET_ROOT").map(std::path::PathBuf::from) else {
            eprintln!("skipped: set SKATE3_ASSET_ROOT to the converted assets");
            return;
        };
        let c = Collections::load(&root).expect("stock collections");
        let base = load(&c, BASE).expect("base record");
        assert_eq!(base, ColourMatrixRecord::default());
        let z = load(&c, ZOMBIE).expect("zombie record");
        eprintln!("{z:?}");
        assert_eq!((z.near.multiply, z.near.saturation, z.near.contrast), ([2.0, 1.4, 1.0], 0.5, 1.3));
        assert_eq!((z.far.contrast, z.far.add, z.distance_near, z.distance_far), (1.5, [-0.4; 3], 50.0, 100.0));
        assert_eq!(bloom_colour(&c), Ok([0.0, 0.5, 1.0]), "f_bloomColor.rgb = this + 1 (b679)");
    }

    /// The reel transition's bloom: no step at rest; w = clamp01(2 x) toward the luma in the bloom color while it
    /// runs, so the frame is fully there from x = 0.5.
    #[test]
    fn the_reel_transition_lifts_the_bloom_weight() {
        let mut app = App::new();
        app.insert_resource(ColourRecords { bloom_colour: Some(Vec3::new(1.0, 1.5, 2.0)), ..default() })
            .init_resource::<ColourGrade>()
            .init_resource::<crate::replay::Replay>()
            .add_systems(Update, update_grade);
        let weight = |app: &mut App, x: Option<crate::replay::ReelPhase>| {
            let mut r = app.world_mut().resource_mut::<crate::replay::Replay>();
            r.active = x.is_some();
            r.reel_transition = x;
            app.update();
            app.world().resource::<ColourGrade>().rows[9]
        };
        use crate::replay::ReelPhase::{In, Out};
        assert_eq!(weight(&mut app, None), Vec4::ZERO);
        assert!((weight(&mut app, Some(In(0.25))) - Vec4::new(1.0, 1.5, 2.0, 0.5)).length() < 1e-5);
        assert_eq!(weight(&mut app, Some(In(0.75))).w, 1.0);
        assert!((weight(&mut app, Some(Out(0.8))).w - 0.4).abs() < 1e-5);
    }
}
