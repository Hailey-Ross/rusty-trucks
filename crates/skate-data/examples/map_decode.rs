//! Data-only package timing tool. Does not initialize the engine or simulate.
use std::{path::Path, time::Instant};
fn main() {
    for path in std::env::args().skip(1) {
        for pass in 0..3 {
            let start = Instant::now();
            let bytes = std::fs::read(Path::new(&path)).expect("read map");
            let read = start.elapsed();
            let start = Instant::now();
            let map = skate_data::skate_map::SkateMap::parse(&bytes).expect("decode map");
            println!("DATA_DECODE name={:?} pass={} read_ms={} parse_ms={} textures={} vertices={}",
                map.name, pass, read.as_millis(), start.elapsed().as_millis(), map.textures.len(), map.geometry.vertices.len());
        }
    }
}
