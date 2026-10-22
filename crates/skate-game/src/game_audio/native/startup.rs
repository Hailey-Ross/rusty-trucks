//! The first start's boot and player banks, read and decoded on a thread of their own while the
//! scene builds. `Native::start_from` used to read them on the game thread in frame 0: about
//! 2,750 WAVs (85 MB), 10.8 s on a Switch, after the 19 s scene build.
//!
//! - Audio `setup` (Startup, before `world::spawn`) starts the thread with the startup library's
//!   sources ([`begin`]).
//! - The thread runs the loads the game thread would run ([`BankSource::load`],
//!   [`SpliceSource::load`], [`wheels_load`]) and keeps the ones that loaded, each with its source.
//! - The first start waits for it to finish (`content::frame` checks [`EarlyLoads::finished`]
//!   each frame) and `native::launch` then joins it: no frame waits for the loads. On a Switch
//!   whose start map loads in the background they take about 30 s, and joining mid-load held the
//!   first frame for 24 s. `start_from` takes a result only when its source is the one the
//!   current library names: mod overlays merged before the first start change a bank's source,
//!   and that bank loads as before.
//! - Only the data moves: `Runtime::load_bank`, the Splice installs and the streams run where and
//!   in the order they did, so the runtime sees exactly what it saw before.
//! - `SKATE_AUDIO_EARLY_LOAD=0` turns it off.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Instant;

use bevy::prelude::*;
use skate_audio::formats::Bank;
use skate_audio::mixer::Pcm;
use skate_audio::splice::SpliceBank;

use super::super::Library;
use super::super::library::{BankPcm, BankSource, SpliceSource, wheels_load};
use super::super::player_audio;

/// The loading thread, from Startup until the first start joins it.
#[derive(Resource)]
pub(crate) struct EarlyLoads {
    thread: JoinHandle<Loaded>,
    started: Instant,
}

/// What the thread loaded, each with the source it read.
#[derive(Default)]
pub(crate) struct Loaded {
    banks: HashMap<String, (BankSource, Bank, BankPcm)>,
    splice: HashMap<String, (SpliceSource, SpliceBank, BankPcm)>,
    wheels: HashMap<String, (PathBuf, Arc<Pcm>)>,
    used: usize,
    stale: usize,
}

/// Start the thread for the startup library. None (everything loads in frame 0, as before): turned
/// off, an install the runtime can't start on, or no thread.
pub(crate) fn begin(library: &Library) -> Option<EarlyLoads> {
    if std::env::var_os("SKATE_AUDIO_EARLY_LOAD").is_some_and(|v| v == "0") {
        info!("AUDIO_EARLY_LOAD off (SKATE_AUDIO_EARLY_LOAD=0): the start-up banks load in frame 0");
        return None;
    }
    let files = library.aems();
    if files.projects.is_empty() {
        return None;
    }
    // `start_from`'s loads: the boot utilities, and with a MixMap (the player) the components'
    // banks, the optional components' banks, the Splice banks and the wheel recordings.
    let player = files.mixmap.is_some();
    let mut stems = vec!["emitter_utility"];
    if player {
        use player_audio::{BANKS, RATTLE_BANKS, ROLLING_BANKS, SLIDE_BANKS, TREATMENT_BANKS, TRICKS_BANKS};
        stems.extend([BANKS, ROLLING_BANKS, RATTLE_BANKS, SLIDE_BANKS, TRICKS_BANKS, TREATMENT_BANKS].concat());
    }
    stems.push(skate_audio::player::seams::UTILITY_BANK);
    let mut banks: Vec<BankSource> = Vec::new();
    for stem in stems {
        if banks.iter().all(|b| b.stem() != stem) {
            if let Ok(source) = library.bank_source(stem) {
                banks.push(source);
            }
        }
    }
    let (splice, wheels): (Vec<_>, Vec<_>) = if player {
        (
            player_audio::SPLICE_BANKS.iter().filter_map(|&stem| Some((stem.to_owned(), library.splice_source(stem)?))).collect(),
            player_audio::WHEEL_STREAMS.iter().filter_map(|&name| Some((name.to_owned(), library.wheels_source(name)?))).collect(),
        )
    } else {
        (Vec::new(), Vec::new())
    };
    let started = Instant::now();
    match std::thread::Builder::new().name("audio-early-load".into()).spawn(move || load(banks, splice, wheels, started)) {
        Ok(thread) => Some(EarlyLoads { thread, started }),
        Err(e) => {
            warn!("AUDIO_EARLY_LOAD no thread ({e}): the start-up banks load in frame 0");
            None
        }
    }
}

/// The thread: each load in turn. A failed or panicking load is left to the game thread, which
/// then fails the same way it always did.
fn load(banks: Vec<BankSource>, splice: Vec<(String, SpliceSource)>, wheels: Vec<(String, PathBuf)>, started: Instant) -> Loaded {
    let mut loaded = Loaded::default();
    let (mut files, mut bytes) = (0usize, 0usize);
    for source in banks {
        if let Ok(Ok((bank, pcm))) = std::panic::catch_unwind(|| source.load()) {
            files += 1 + pcm.len();
            bytes += pcm_bytes(&pcm);
            loaded.banks.insert(source.stem().to_owned(), (source, bank, pcm));
        }
    }
    for (stem, source) in splice {
        if let Ok(Some((bank, pcm))) = std::panic::catch_unwind(|| source.load()) {
            files += 1 + pcm.len();
            bytes += pcm_bytes(&pcm);
            loaded.splice.insert(stem, (source, bank, pcm));
        }
    }
    for (name, file) in wheels {
        if let Ok(Some(pcm)) = std::panic::catch_unwind(|| wheels_load(&file)) {
            files += 1;
            bytes += pcm_len(&pcm);
            loaded.wheels.insert(name, (file, pcm));
        }
    }
    info!(
        "AUDIO_EARLY_LOAD loaded {} banks, {} Splice banks and {} wheel recordings ({files} files, {:.1} MiB of PCM) in {} ms",
        loaded.banks.len(),
        loaded.splice.len(),
        loaded.wheels.len(),
        bytes as f64 / (1024.0 * 1024.0),
        started.elapsed().as_millis()
    );
    loaded
}

fn pcm_bytes(pcm: &BankPcm) -> usize {
    pcm.iter().flatten().map(|p| pcm_len(p)).sum()
}

fn pcm_len(pcm: &Pcm) -> usize {
    pcm.channels.iter().map(Vec::len).sum::<usize>() * size_of::<f32>()
}

impl EarlyLoads {
    /// Whether the thread is done, so [`join`](Self::join) won't wait.
    pub(crate) fn finished(&self) -> bool {
        self.thread.is_finished()
    }

    /// Wait for the thread (while it is still loading) and take what it loaded; nothing when it
    /// panicked (everything then loads as before).
    pub(crate) fn join(self) -> Loaded {
        let waiting = Instant::now();
        let loaded = self.thread.join().unwrap_or_default();
        info!(
            "AUDIO_EARLY_LOAD joined {} ms after the thread started; the first start waited {} ms for it",
            self.started.elapsed().as_millis(),
            waiting.elapsed().as_millis()
        );
        loaded
    }
}

impl Loaded {
    /// The bank, if the thread loaded it from `source` (the one the current library names).
    pub(crate) fn take_bank(&mut self, source: &BankSource) -> Option<(Bank, BankPcm)> {
        let (from, bank, pcm) = self.banks.remove(source.stem())?;
        self.matched(from == *source).then_some((bank, pcm))
    }

    /// The Splice bank, if the thread loaded it from `source`.
    pub(crate) fn take_splice(&mut self, stem: &str, source: &SpliceSource) -> Option<(SpliceBank, BankPcm)> {
        let (from, bank, pcm) = self.splice.remove(stem)?;
        self.matched(from == *source).then_some((bank, pcm))
    }

    /// The wheel recording, if the thread loaded it from `file`.
    pub(crate) fn take_wheel(&mut self, name: &str, file: &Path) -> Option<Arc<Pcm>> {
        let (from, pcm) = self.wheels.remove(name)?;
        self.matched(from.as_path() == file).then_some(pcm)
    }

    fn matched(&mut self, same: bool) -> bool {
        if same {
            self.used += 1;
        } else {
            self.stale += 1;
        }
        same
    }

    /// After the first start: what it took (the rest is freed here).
    pub(crate) fn finish(self) {
        let unused = self.banks.len() + self.splice.len() + self.wheels.len();
        if self.used + self.stale + unused > 0 {
            info!("AUDIO_EARLY_LOAD the first start used {} loads; {} had another source (mods: loaded again), {} unused", self.used, self.stale, unused);
        }
    }
}
