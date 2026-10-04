//! Validate a mod package before shipping it:
//!
//!   check_mod <package-folder-or-zip> [--install <assets folder>] [--with <other package>…]
//!
//! Checks `mod.json`, the Lua entry's syntax and, when the package has an `audio.json`, the audio
//! content overlay in depth (schema, every WAV / bank / Splice tree / MixMap / grain member it
//! names). With `--install` (the game's `assets` folder) the overlay is also merged over the
//! install's audio manifest, as the game does, to list identities the install does not have and
//! conflicts with the `--with` packages (merged in mod-id order, the first owner wins).
fn main() {
    let mut args = std::env::args().skip(1);
    let mut path = None;
    let mut install = None;
    let mut others = Vec::new();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--install" => install = args.next(),
            "--with" => others.extend(args.next()),
            _ => path = Some(a),
        }
    }
    let path = path.expect("usage: check_mod <package-folder-or-zip> [--install <assets>] [--with <package>…]");
    let (manifest, audio) = match skate_mods::validate_package_content(std::path::Path::new(&path)) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("FAIL: {e}");
            std::process::exit(1);
        }
    };
    println!("OK {} api={} entry={}", manifest.id, manifest.api, manifest.entry);
    let Some(audio) = audio else { return };
    println!("audio.json: {} MiB of PCM", audio.pcm_bytes as f64 / (1024.0 * 1024.0));
    for line in audio.overlay.summary() {
        println!("  {line}");
    }
    let Some(install) = install else { return };
    let file = std::path::Path::new(&install).join("private/audio/audio_manifest.json");
    let mut m: serde_json::Value = match std::fs::read(&file).map_err(|e| e.to_string()).and_then(|b| serde_json::from_slice(&b).map_err(|e| e.to_string())) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("FAIL: {}: {e}", file.display());
            std::process::exit(1);
        }
    };
    let mut overlays = vec![(manifest.id.clone(), audio.overlay)];
    for other in &others {
        match skate_mods::validate_package_content(std::path::Path::new(other)) {
            Ok((m, Some(a))) if !overlays.iter().any(|(id, _)| *id == m.id) => overlays.push((m.id, a.overlay)),
            Ok((m, Some(_))) => println!("  ({} is already in the list)", m.id),
            Ok((m, None)) => println!("  ({} has no audio.json)", m.id),
            Err(e) => println!("  ({other}: {e})"),
        }
    }
    overlays.sort_by(|a, b| a.0.cmp(&b.0));
    let sources: Vec<_> = overlays.iter().map(|(id, o)| skate_mods::audio_merge::Source { id, overlay: o }).collect();
    let report = skate_mods::audio_merge::merge(&mut m, &sources);
    let mine = |owner: &str| owner == manifest.id;
    let warnings: Vec<_> = report.warnings.iter().filter(|w| mine(&w.owner)).collect();
    let conflicts: Vec<_> = report.conflicts.iter().filter(|c| mine(&c.owner)).collect();
    println!("install: {} identities changed, {} warnings, {} conflicts", report.claimed.values().filter(|o| mine(o)).count(), warnings.len(), conflicts.len());
    for w in warnings {
        println!("  warning: {}", w.text);
    }
    for c in conflicts {
        println!("  conflict: {}", c.text);
    }
}
