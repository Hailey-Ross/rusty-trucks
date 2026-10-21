//! World texture pages kept on the SD card between launches. Building University's eight BC3
//! pages takes 19 s on a Switch (resampling, mipmaps and range-fit BC3 on three cores); reading
//! their 235 MiB back from the SD card should take a few seconds.
//!
//! - One file per map, slab and page class, `<SD root>/cache/<map>-s<slab>-p<class>.page`, so a
//!   map keeps at most its pages' worth of files and a new key overwrites the old one.
//! - The key hashes what the page is built from: every layer's source texture (its size and
//!   RGBA), the page's size, mip levels, format and layer count, and [`VERSION`]. A changed map,
//!   texture or option misses and rebuilds.
//! - The file also holds its payload's hash, so a short or damaged file misses too.
//! - Only BC3 pages are kept: RGBA8 pages are four times the size and quicker to build than read.
//! - A miss builds the page as before and writes it (to a temporary file, then renamed), so a
//!   map's first launch takes a few seconds longer.
//! - On by default on the Switch, off on a PC. `SKATE_PAGE_CACHE=0` turns it off, `=1` on;
//!   `SKATE_PAGE_CACHE_VERIFY=1` builds every page anyway and compares it with the cached copy.
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use bevy::prelude::*;

/// Bump when the page bytes change for the same source textures: the resampling, the mip chain,
/// the BC3 encoder or its parameters, or the layout (retail_render.rs `PageFormat`). Version 2
/// also moved the key's texture identity from decoded pixels to compressed payloads, which
/// changes what a cached entry means (findings 58 §3.6).
const VERSION: u32 = 2;
const MAGIC: [u8; 8] = *b"SK8PAGE\0";
const HEADER: usize = 40;
/// Read and write in pieces this size.
const CHUNK: usize = 8 << 20;

/// Where one map's pages are cached.
pub(crate) struct PageCache {
    dir: PathBuf,
    map: String,
    verify: bool,
}

/// Why a page was not read from the cache.
pub(crate) enum Miss {
    NoFile,
    Stale,
    Damaged(String),
}

impl std::fmt::Display for Miss {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoFile => f.write_str("not cached yet"),
            Self::Stale => f.write_str("the cached copy is of other textures or another build"),
            Self::Damaged(why) => write!(f, "the cached copy is damaged ({why})"),
        }
    }
}

impl PageCache {
    /// The cache for the map `name` beside the assets (`<asset_root>/../cache`), when it is on.
    pub(crate) fn for_map(asset_root: &Path, name: &str) -> Option<Self> {
        let on = match std::env::var("SKATE_PAGE_CACHE").as_deref() {
            Ok("0") => false,
            Ok("1") => true,
            _ => cfg!(target_os = "horizon"),
        };
        if !on {
            return None;
        }
        let root = asset_root.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        let map: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
        let verify = std::env::var("SKATE_PAGE_CACHE_VERIFY").is_ok_and(|v| v == "1");
        Some(Self { dir: root.join("cache"), map, verify })
    }

    /// Whether every page is built anyway and compared with its cached copy.
    pub(crate) fn verify(&self) -> bool {
        self.verify
    }

    pub(crate) fn path(&self, slab: usize, class: usize) -> PathBuf {
        self.dir.join(format!("{}-s{slab}-p{class}.page", self.map))
    }

    /// The cached page, if its file holds `len` bytes for `key`.
    pub(crate) fn read(&self, slab: usize, class: usize, key: u64, len: usize) -> Result<Vec<u8>, Miss> {
        let mut file = match std::fs::File::open(self.path(slab, class)) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(Miss::NoFile),
            Err(e) => return Err(Miss::Damaged(e.to_string())),
        };
        let mut header = [0u8; HEADER];
        file.read_exact(&mut header).map_err(|e| Miss::Damaged(e.to_string()))?;
        let word = |at: usize| u64::from_le_bytes(header[at..at + 8].try_into().unwrap());
        if header[..8] != MAGIC || header[8..12] != VERSION.to_le_bytes() || word(16) != key {
            return Err(Miss::Stale);
        }
        if word(24) != len as u64 {
            return Err(Miss::Damaged(format!("{} bytes, not {len}", word(24))));
        }
        let mut bytes = vec![0u8; len];
        for chunk in bytes.chunks_mut(CHUNK) {
            file.read_exact(chunk).map_err(|e| Miss::Damaged(e.to_string()))?;
        }
        if hash(PAYLOAD_SEED, &bytes) != word(32) {
            return Err(Miss::Damaged("its bytes don't match their hash".into()));
        }
        Ok(bytes)
    }

    /// Keep a built page. A failure (a full SD card) is logged and the game goes on without it.
    pub(crate) fn write(&self, slab: usize, class: usize, key: u64, bytes: &[u8]) {
        let started = std::time::Instant::now();
        let path = self.path(slab, class);
        let temporary = path.with_extension("tmp");
        let result = (|| -> std::io::Result<()> {
            std::fs::create_dir_all(&self.dir)?;
            let mut file = std::fs::File::create(&temporary)?;
            let mut header = [0u8; HEADER];
            header[..8].copy_from_slice(&MAGIC);
            header[8..12].copy_from_slice(&VERSION.to_le_bytes());
            header[16..24].copy_from_slice(&key.to_le_bytes());
            header[24..32].copy_from_slice(&(bytes.len() as u64).to_le_bytes());
            header[32..40].copy_from_slice(&hash(PAYLOAD_SEED, bytes).to_le_bytes());
            file.write_all(&header)?;
            for chunk in bytes.chunks(CHUNK) {
                file.write_all(chunk)?;
            }
            // Not every file system flushes on request; a file cut short misses by its hash.
            let _ = file.sync_all();
            drop(file);
            // Horizon's rename won't replace a file.
            let _ = std::fs::remove_file(&path);
            std::fs::rename(&temporary, &path)
        })();
        match result {
            Ok(()) => info!(
                "PAGE_CACHE wrote page {class} of slab {slab} to {} ({:.1} MiB in {} ms)",
                path.display(),
                bytes.len() as f64 / (1024. * 1024.),
                started.elapsed().as_millis()
            ),
            Err(e) => {
                let _ = std::fs::remove_file(&temporary);
                warn!("PAGE_CACHE could not write {}: {e}; the page is built again at the next launch", path.display());
            }
        }
    }
}

const KEY_SEED: u64 = 0x5348_4B38_5041_4745;
const PAYLOAD_SEED: u64 = 0x7061_796C_6F61_6421;
const MULTIPLIER: u64 = 0x9E37_79B9_7F4A_7C15;

fn mix(h: u64, word: u64) -> u64 {
    (h.rotate_left(23) ^ word).wrapping_mul(MULTIPLIER)
}

/// A 64-bit hash of `bytes`: four lanes of rotate, xor and multiply by an odd constant, so a
/// change to any one word always changes the result. Not cryptographic; it tells versions of a
/// texture apart.
pub(crate) fn hash(seed: u64, bytes: &[u8]) -> u64 {
    let mut lanes = [seed, seed ^ MULTIPLIER, seed.rotate_left(16), !seed];
    let mut blocks = bytes.chunks_exact(32);
    for block in &mut blocks {
        for (lane, word) in lanes.iter_mut().zip(block.chunks_exact(8)) {
            *lane = mix(*lane, u64::from_le_bytes(word.try_into().unwrap()));
        }
    }
    let mut h = mix(seed, bytes.len() as u64);
    for lane in lanes {
        h = mix(h, lane);
    }
    let mut words = blocks.remainder().chunks_exact(8);
    for word in &mut words {
        h = mix(h, u64::from_le_bytes(word.try_into().unwrap()));
    }
    for &byte in words.remainder() {
        h = mix(h, u64::from(byte));
    }
    h ^= h >> 32;
    h = h.wrapping_mul(MULTIPLIER);
    h ^ (h >> 29)
}

/// A page's key: `params` (its size, levels, format, layer count) and its layers' source textures
/// in order, each `(width, height, compressed payload)`, hashed on `workers` threads.
pub(crate) fn key(params: &[u64], layers: &[(u32, u32, &[u8])], workers: usize) -> u64 {
    let mut hashes = vec![0u64; layers.len()];
    {
        let jobs = Mutex::new(layers.iter().zip(hashes.iter_mut()));
        let work = || loop {
            let Some((&(width, height, rgba), out)) = jobs.lock().unwrap().next() else { break };
            *out = hash(mix(mix(KEY_SEED, u64::from(width)), u64::from(height)), rgba);
        };
        std::thread::scope(|scope| {
            for _ in 1..workers.min(layers.len()) {
                scope.spawn(work);
            }
            work();
        });
    }
    let mut key = mix(KEY_SEED, u64::from(VERSION));
    for &p in params {
        key = mix(key, p);
    }
    for h in hashes {
        key = mix(key, h);
    }
    key
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_sees_every_word_and_the_length() {
        let bytes: Vec<u8> = (0..1000u32).map(|i| (i * 7) as u8).collect();
        let base = hash(1, &bytes);
        for at in [0, 7, 8, 31, 32, 500, 991, 999] {
            let mut changed = bytes.clone();
            changed[at] ^= 1;
            assert_ne!(hash(1, &changed), base, "byte {at}");
        }
        assert_ne!(hash(1, &bytes[..999]), base);
        assert_ne!(hash(2, &bytes), base);
        assert_eq!(hash(1, &bytes), base);
    }

    #[test]
    fn a_page_reads_back_and_a_stale_or_damaged_one_misses() {
        let dir = std::env::temp_dir().join(format!("sk8-page-cache-{}", std::process::id()));
        let cache = PageCache { dir: dir.clone(), map: "Test".into(), verify: false };
        let bytes: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
        assert!(matches!(cache.read(0, 2, 42, bytes.len()), Err(Miss::NoFile)));
        cache.write(0, 2, 42, &bytes);
        assert_eq!(cache.read(0, 2, 42, bytes.len()).ok().as_deref(), Some(&bytes[..]));
        assert!(matches!(cache.read(0, 2, 43, bytes.len()), Err(Miss::Stale)));
        assert!(matches!(cache.read(0, 2, 42, bytes.len() - 1), Err(Miss::Damaged(_))));
        // A newer key replaces the file.
        cache.write(0, 2, 43, &bytes[..50]);
        assert_eq!(cache.read(0, 2, 43, 50).ok().as_deref(), Some(&bytes[..50]));
        // A flipped payload byte misses.
        let path = cache.path(0, 2);
        let mut file = std::fs::read(&path).unwrap();
        file[HEADER + 10] ^= 0xff;
        std::fs::write(&path, file).unwrap();
        assert!(matches!(cache.read(0, 2, 43, 50), Err(Miss::Damaged(_))));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_key_follows_the_textures_and_the_params() {
        let (a, b): (&[u8], &[u8]) = (&[1u8; 4096], &[2u8; 4096]);
        let base = key(&[512, 512, 10], &[(32, 32, a), (32, 32, b)], 3);
        assert_eq!(key(&[512, 512, 10], &[(32, 32, a), (32, 32, b)], 1), base);
        assert_ne!(key(&[512, 512, 10], &[(32, 32, b), (32, 32, a)], 3), base);
        assert_ne!(key(&[512, 512, 9], &[(32, 32, a), (32, 32, b)], 3), base);
        assert_ne!(key(&[512, 512, 10], &[(64, 16, a), (32, 32, b)], 3), base);
    }
}
