//! The decoded audio described by private/audio/audio_manifest.json. Clips are
//! read on first use (sample banks are small; an ambience bed is ~25 MB of
//! PCM) and kept until released.
use bevy::prelude::*;
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashMap},
    path::{Component, Path, PathBuf},
    sync::Arc,
};

const MANIFEST_VERSION: u32 = 3;

#[derive(Debug, Deserialize)]
pub(crate) struct Entry {
    pub file: String,
}

/// A rolling grain: its slow-to-fast recording cut into loopable speed bands.
#[derive(Debug, Deserialize)]
struct Grain {
    bands: Vec<Entry>,
}

/// One layer of a retail patch record: members as
/// (sample, gain, gain range, pitch, probability) — see audio_formats.splc_patches.
#[derive(Debug, Deserialize)]
pub(crate) struct Group {
    pub mode: u8,
    pub members: Vec<(usize, f32, f32, f32, f32)>,
}

/// Retail SPLC patch tree of one bank: records (layers played together) and
/// containers (pick one record).
#[derive(Debug, Default, Deserialize)]
pub(crate) struct Patches {
    pub records: Vec<Vec<Group>>,
    pub containers: Vec<Vec<usize>>,
}

#[derive(Debug, Deserialize)]
struct Manifest {
    version: u32,
    ambience: BTreeMap<String, Entry>,
    grains: BTreeMap<String, Grain>,
    wheels: BTreeMap<String, Entry>,
    banks: BTreeMap<String, Vec<Entry>>,
    #[serde(default)]
    patches: BTreeMap<String, Patches>,
}

/// A loaded sound. `key` identifies the file for per-sound voice limits;
/// `peak` is its loudest sample (0..1 of full scale), measured on load.
#[derive(Clone)]
pub(crate) struct Clip {
    pub handle: Handle<AudioSource>,
    pub key: Arc<str>,
    pub peak: f32,
}

/// Loudest sample of a PCM16 WAV as a fraction of full scale (1.0 if unreadable).
pub(crate) fn wav_peak(bytes: &[u8]) -> f32 {
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        let body = at + 8;
        if &bytes[at..at + 4] == b"data" {
            let data = &bytes[body..(body + size).min(bytes.len())];
            let max = data.chunks_exact(2).map(|s| i16::from_le_bytes([s[0], s[1]]).unsigned_abs()).max().unwrap_or(0);
            return (max as f32 / 32768.0).max(1e-3);
        }
        at = body + size + (size & 1);
    }
    1.0
}

#[derive(Resource)]
pub(crate) struct Library {
    root: PathBuf,
    manifest: Manifest,
    loaded: HashMap<String, Clip>,
    failed: std::collections::HashSet<String>,
}

fn safe_relative(file: &str) -> bool {
    let path = Path::new(file);
    // ':' also rules out drive prefixes on platforms that would parse "C:" as a name.
    !file.is_empty() && !file.contains(':') && path.components().all(|c| matches!(c, Component::Normal(_)))
}

impl Library {
    pub(crate) fn load(asset_root: &Path) -> Result<Self, String> {
        let root = asset_root.join("private/audio");
        let path = root.join("audio_manifest.json");
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        if manifest.version != MANIFEST_VERSION {
            return Err(format!("{}: unsupported version {}", path.display(), manifest.version));
        }
        let files = manifest.ambience.values().chain(manifest.grains.values().flat_map(|g| &g.bands)).chain(manifest.wheels.values())
            .chain(manifest.banks.values().flatten());
        if let Some(bad) = files.map(|e| &e.file).find(|f| !safe_relative(f)) {
            return Err(format!("{}: invalid file path {bad:?}", path.display()));
        }
        info!(
            "Game audio: {} ambience beds, {} rolling grains, {} sample banks",
            manifest.ambience.len(), manifest.grains.len(), manifest.banks.len()
        );
        Ok(Self { root, manifest, loaded: HashMap::new(), failed: Default::default() })
    }

    fn clip(&mut self, assets: &mut Assets<AudioSource>, file: &str) -> Option<Clip> {
        if let Some(clip) = self.loaded.get(file) {
            return Some(clip.clone());
        }
        if self.failed.contains(file) {
            return None;
        }
        match std::fs::read(self.root.join(file)) {
            Ok(bytes) => {
                let peak = wav_peak(&bytes);
                let clip = Clip { handle: assets.add(AudioSource { bytes: bytes.into() }), key: file.into(), peak };
                self.loaded.insert(file.to_owned(), clip.clone());
                Some(clip)
            }
            Err(error) => {
                // Once per file: a damaged install must not spam the log every frame.
                warn!("Game audio {file}: {error}");
                self.failed.insert(file.to_owned());
                None
            }
        }
    }

    pub(crate) fn ambience(&mut self, assets: &mut Assets<AudioSource>, name: &str) -> Option<Clip> {
        let entry = self.manifest.ambience.get(name)?;
        let file = entry.file.clone();
        self.clip(assets, &file)
    }

    /// Number of speed bands of a rolling grain (0 if absent).
    pub(crate) fn grain_bands(&self, name: &str) -> usize {
        self.manifest.grains.get(name).map_or(0, |g| g.bands.len())
    }

    pub(crate) fn grain(&mut self, assets: &mut Assets<AudioSource>, name: &str, band: usize) -> Option<Clip> {
        let entry = self.manifest.grains.get(name)?.bands.get(band)?;
        let file = entry.file.clone();
        self.clip(assets, &file)
    }

    pub(crate) fn wheels(&mut self, assets: &mut Assets<AudioSource>, name: &str) -> Option<Clip> {
        let entry = self.manifest.wheels.get(name)?;
        let file = entry.file.clone();
        self.clip(assets, &file)
    }

    pub(crate) fn sample(&mut self, assets: &mut Assets<AudioSource>, bank: &str, index: usize) -> Option<Clip> {
        let entry = self.manifest.banks.get(bank)?.get(index)?;
        let file = entry.file.clone();
        self.clip(assets, &file)
    }

    /// Read every grain band, wheel spin and listed bank sample now, so
    /// gameplay never waits on the disk (~20 MB). Returns the clip count.
    pub(crate) fn preload(&mut self, assets: &mut Assets<AudioSource>, samples: &[(&str, &[usize])]) -> usize {
        let mut files: Vec<String> = self.manifest.grains.values().flat_map(|g| &g.bands)
            .chain(self.manifest.wheels.values()).map(|e| e.file.clone()).collect();
        for (bank, indices) in samples {
            if let Some(entries) = self.manifest.banks.get(*bank) {
                files.extend(indices.iter().filter_map(|&i| entries.get(i)).map(|e| e.file.clone()));
            }
        }
        files.iter().filter(|file| self.clip(assets, file).is_some()).count()
    }

    /// A bank's retail patch tree (SPLC banks only).
    pub(crate) fn patches(&self, bank: &str) -> Option<&Patches> {
        self.manifest.patches.get(bank)
    }

    /// Every sample patch `id` of `bank` can play (all records of a container,
    /// every layer member), for preloading.
    pub(crate) fn patch_samples(&self, bank: &str, id: usize) -> Vec<usize> {
        let Some(patches) = self.patches(bank) else { return Vec::new() };
        let records = patches.records.len();
        let ids = if id < records { vec![id] } else { patches.containers.get(id - records).cloned().unwrap_or_default() };
        let mut samples: Vec<usize> = ids.iter().filter_map(|&r| patches.records.get(r)).flatten()
            .flat_map(|group| group.members.iter().map(|member| member.0)).collect();
        samples.sort_unstable();
        samples.dedup();
        samples
    }

    /// Drop a clip's PCM once nothing plays it (rodio keeps its own copy while playing).
    pub(crate) fn release(&mut self, assets: &mut Assets<AudioSource>, clip: &Clip) {
        if self.loaded.remove(&*clip.key).is_some() {
            assets.remove(clip.handle.id());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_peak_reads_the_data_chunk() {
        let mut wav = b"RIFF\x00\x00\x00\x00WAVEfmt \x10\x00\x00\x00".to_vec();
        wav.extend([1, 0, 1, 0, 0x80, 0xbb, 0, 0, 0, 0x77, 1, 0, 2, 0, 16, 0]);
        wav.extend(b"LIST\x03\x00\x00\x00abc\x00");
        wav.extend(b"data\x06\x00\x00\x00");
        for s in [100i16, -8192, 50] {
            wav.extend(s.to_le_bytes());
        }
        assert_eq!(wav_peak(&wav), 0.25);
        assert_eq!(wav_peak(b"RIFF"), 1.0);
    }

    #[test]
    fn manifest_paths_must_stay_inside_the_audio_folder() {
        assert!(safe_relative("banks/GRINDS/0001.wav"));
        for bad in ["", "../x.wav", "/x.wav", "C:/x.wav", "banks/../../x.wav"] {
            assert!(!safe_relative(bad), "{bad}");
        }
    }

    #[test]
    fn loads_on_demand_and_reports_missing_files_once() {
        let dir = std::env::temp_dir().join(format!("skate-audio-library-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("private/audio/banks/x")).unwrap();
        std::fs::write(dir.join("private/audio/banks/x/0000.wav"), b"RIFF").unwrap();
        std::fs::write(
            dir.join("private/audio/audio_manifest.json"),
            r#"{"version":3,"ambience":{},"grains":{},"wheels":{},
                "banks":{"x":[{"file":"banks/x/0000.wav","seconds":0.1},{"file":"banks/x/missing.wav","seconds":0.1}]}}"#,
        )
        .unwrap();
        let mut library = Library::load(&dir).unwrap();
        let mut assets = Assets::<AudioSource>::default();
        let clip = library.sample(&mut assets, "x", 0).unwrap();
        assert_eq!(&*clip.key, "banks/x/0000.wav");
        assert!(library.sample(&mut assets, "x", 1).is_none());
        assert!(library.failed.contains("banks/x/missing.wav"));
        assert!(library.sample(&mut assets, "x", 2).is_none());
        library.release(&mut assets, &clip);
        assert!(assets.get(clip.handle.id()).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
