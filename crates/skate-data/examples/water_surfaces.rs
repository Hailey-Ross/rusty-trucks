//! Lists the retail collision surface types in SKATE maps and details the
//! water surfaces (surface type 12, `(surface >> 7) & 31`).
//! Usage: cargo run -p skate-data --example water_surfaces -- <map.skate>...
use std::collections::BTreeMap;

const WATER: u16 = 12;

#[derive(Default)]
struct Water {
    triangles: usize,
    min: [f32; 3],
    max: [f32; 3],
    heights: BTreeMap<i32, usize>,
    surfaces: BTreeMap<u16, usize>,
    meshes: BTreeMap<String, usize>,
    flat: usize,
    one_sided: usize,
}

fn main() {
    let mut failed = false;
    for path in std::env::args_os().skip(1) {
        let map = match skate_data::skate_map::SkateMap::load(std::path::Path::new(&path)) {
            Ok(map) => map,
            Err(e) => {
                eprintln!("{}: {e}", path.to_string_lossy());
                failed = true;
                continue;
            }
        };
        render_water(&map);
        let Some(archive) = map.extensions.iter().find(|e| e.tag == *b"RWCM") else {
            println!("{}: no RWCM collision", map.name);
            continue;
        };
        let mut types = [0_usize; 32];
        let mut water = Water {
            min: [f32::MAX; 3],
            max: [f32::MIN; 3],
            ..Default::default()
        };
        let mut streams = BTreeMap::<String, usize>::new();
        // Surface IDs of the lowest geometry (harbour/sea beds below y = -1).
        let mut low = BTreeMap::<u16, (usize, f32)>::new();
        let result = skate_data::retail_collision::visit_clusters(&archive.payload, |name, cluster| {
            // Tile streams are cSim_<x>_<z>_<lod>.xsf; summarise by LOD/kind.
            let stream = name.split('#').next().unwrap_or(name);
            let kind = stream
                .split('_')
                .filter(|part| part.parse::<i32>().is_err())
                .collect::<Vec<_>>()
                .join("_");
            *streams.entry(kind).or_default() += cluster.len();
            for t in cluster {
                let kind = (t.surface >> 7) & 31;
                let top = t.points.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
                if top < -1. {
                    let entry = low.entry(t.surface).or_insert((0, f32::MAX));
                    entry.0 += 1;
                    entry.1 = entry.1.min(top);
                }
                types[usize::from(kind)] += 1;
                if kind != WATER {
                    continue;
                }
                water.triangles += 1;
                if std::env::var_os("WATER_POINTS").is_some() {
                    let c: [f32; 3] = std::array::from_fn(|axis| {
                        t.points.iter().map(|p| p[axis]).sum::<f32>() / 3.
                    });
                    println!("  water centroid {:.1} {:.2} {:.1} ({name})", c[0], c[1], c[2]);
                }
                *water.surfaces.entry(t.surface).or_default() += 1;
                *water.meshes.entry(name.to_owned()).or_default() += 1;
                for p in t.points {
                    for axis in 0..3 {
                        water.min[axis] = water.min[axis].min(p[axis]);
                        water.max[axis] = water.max[axis].max(p[axis]);
                    }
                }
                let ys = t.points.map(|p| p[1]);
                let spread = ys.iter().cloned().fold(f32::MIN, f32::max)
                    - ys.iter().cloned().fold(f32::MAX, f32::min);
                if spread < 0.01 {
                    water.flat += 1;
                }
                if t.one_sided {
                    water.one_sided += 1;
                }
                *water.heights.entry((ys[0] * 10.).round() as i32).or_default() += 1;
            }
            Ok(())
        });
        if let Err(e) = result {
            eprintln!("{}: {e}", map.name);
            failed = true;
            continue;
        }
        let histogram: Vec<_> = types
            .iter()
            .enumerate()
            .filter(|(_, n)| **n > 0)
            .map(|(kind, n)| format!("{kind}:{n}"))
            .collect();
        println!("{}: surface types {}", map.name, histogram.join(" "));
        println!("  collision streams (triangles): {streams:?}");
        let low: Vec<_> = low
            .iter()
            .map(|(s, (n, y))| format!("{s}(type {}):{n} min {y:.1}m", (s >> 7) & 31))
            .collect();
        println!("  surfaces below -1 m: {}", low.join(", "));
        if water.triangles == 0 {
            println!("  water: none");
            continue;
        }
        println!(
            "  water: {} triangles ({} flat, {} one-sided), bounds {:?}..{:?}",
            water.triangles, water.flat, water.one_sided, water.min, water.max
        );
        println!("  water surface IDs: {:?}", water.surfaces);
        let heights: Vec<_> = water
            .heights
            .iter()
            .map(|(y, n)| format!("{:.1}m:{n}", *y as f32 / 10.))
            .collect();
        println!("  water heights (first vertex): {}", heights.join(" "));
        let mut meshes: Vec<_> = water.meshes.into_iter().collect();
        meshes.sort_by(|a, b| b.1.cmp(&a.1));
        for (name, n) in meshes.iter().take(12) {
            println!("  water mesh {name}: {n}");
        }
        if meshes.len() > 12 {
            println!("  ... {} more water meshes", meshes.len() - 12);
        }
    }
    if failed {
        std::process::exit(1);
    }
}

/// Height range of render meshes whose retail shader is water.* or ocean.*.
fn render_water(map: &skate_data::skate_map::SkateMap) {
    let shader = |m: &skate_data::skate_map::Material| {
        let bytes = m.retail_definition.as_deref()?;
        ["water.", "ocean."].iter().find_map(|prefix| {
            let at = bytes.windows(prefix.len()).position(|w| w == prefix.as_bytes())?;
            let end = bytes[at..].iter().position(|b| !(b.is_ascii_alphanumeric() || *b == b'.' || *b == b'_'))?;
            Some(String::from_utf8_lossy(&bytes[at..at + end]).into_owned())
        })
    };
    let shaders: Vec<_> = map.materials.iter().map(shader).collect();
    let mut ranges = BTreeMap::<(String, String), ([f32; 3], [f32; 3], usize)>::new();
    for v in &map.geometry.vertices {
        let Some(Some(name)) = shaders.get(v.material as usize) else {
            continue;
        };
        let key = (name.clone(), map.materials[v.material as usize].name.clone());
        let entry = ranges.entry(key).or_insert(([f32::MAX; 3], [f32::MIN; 3], 0));
        for axis in 0..3 {
            entry.0[axis] = entry.0[axis].min(v.position[axis]);
            entry.1[axis] = entry.1[axis].max(v.position[axis]);
        }
        entry.2 += 1;
    }
    for ((shader, material), (min, max, n)) in ranges {
        println!(
            "  render {shader} {material}: {n} vertices, y {:.2}..{:.2}, x {:.0}..{:.0}, z {:.0}..{:.0}",
            min[1], max[1], min[0], max[0], min[2], max[2]
        );
    }
}
