//! The front-end stage (doc 31, Milestone 5a): several retail menu movies running in ONE script
//! VM, like retail's single APT player where FE_root, the screen manager and every screen movie
//! share `_global`. Movies are loaded into clips at runtime (MovieClipLoader.loadClip, used by
//! screenmanager.apt OpenScreen) and clips are created by script (createEmptyMovieClip).
//!
//! The C++ side of retail (screen stack, Game.OnScreen* handlers, CoreMenu, ...) is supplied by a
//! `StageNatives` implementation: the stage only routes calls on the named host objects to it.
//! Movie sources come from a `MovieSource` (the linked retail set with mod overrides layered in,
//! `RetailMenuMovies`), so a mod-replaced screen is what gets loaded.
//!
//! Each loaded movie keeps its own timeline rate (`Movie::milliseconds_per_frame`), so screens
//! run at their authored speed for any engine frame rate.
use crate::{
    apt_imports::library_key,
    apt_movie::{Movie, Pending, CLASS_CONSTRUCTOR},
    apt_vm::{Host, ObjectKind, Value, Vm},
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Host objects the retail C++ registers for the AS side (sub_825A2190 creates ~40: Game,
/// Audio, CoreMenu, ...). Calls on them go to `call`.
pub trait StageNatives {
    fn objects(&self) -> Vec<String>;
    fn call(
        &mut self,
        vm: &mut Vm,
        object: &str,
        method: &str,
        args: Vec<Value>,
    ) -> Result<Value, String>;
}

/// Linked movies by retail key (`apt_imports::library_key`).
pub trait MovieSource {
    fn movie(&self, key: &str) -> Option<Movie>;
}
impl MovieSource for BTreeMap<String, Movie> {
    fn movie(&self, key: &str) -> Option<Movie> {
        self.get(key).cloned()
    }
}

/// One movie playing on the stage.
pub struct Level {
    pub key: String,
    pub movie: Movie,
    /// Milliseconds not yet turned into frames.
    elapsed: f32,
}

/// Timeline rate when a movie header gives none (the retail HUD and most menus use 16 ms;
/// doc 31 "Milestone 5a").
const DEFAULT_MS_PER_FRAME: f32 = 16.0;
/// Frames a single update may advance one movie (a long hitch does not replay seconds of
/// animation at once; NOT RETAIL: retail's catch-up rule is not decoded).
const MAX_FRAMES_PER_UPDATE: usize = 4;
const ACTION_LIMIT: usize = 8192;

struct Load {
    key: String,
    target: usize,
    loader: usize,
}

struct State {
    levels: Vec<Level>,
    loads: VecDeque<Load>,
    /// Clips made by createEmptyMovieClip: id -> (parent, depth).
    dynamic: BTreeMap<usize, (usize, i32)>,
    loader_prototype: usize,
    /// The global `Object` (registerClass lives on it).
    object_class: usize,
    /// registerClass bindings: linkage name -> class function.
    classes: BTreeMap<String, usize>,
    /// Library movies whose root init actions already ran (once per stage, like a library
    /// that stays loaded).
    libraries: BTreeSet<String>,
    native_names: BTreeSet<String>,
    /// Calls nothing handled (method on object kind), for diagnostics and tests.
    pub unhandled: BTreeSet<String>,
    traces: Vec<String>,
}

pub struct Stage {
    pub vm: Vm,
    state: State,
}

struct Bindings<'a> {
    state: &'a mut State,
    natives: &'a mut dyn StageNatives,
}

fn new_clip(vm: &mut Vm, parent: usize, name: &str) -> Result<usize, String> {
    let id = vm.object(ObjectKind::Native("movie:empty".into()));
    let root = vm.get(parent, "_root");
    vm.set(id, "_root", if root == Value::Undefined { Value::Object(parent) } else { root })?;
    vm.set(id, "_parent", Value::Object(parent))?;
    vm.set(id, "_name", Value::Text(name.into()))?;
    vm.set(id, "_x", Value::Number(0.0))?;
    vm.set(id, "_y", Value::Number(0.0))?;
    vm.set(id, "_visible", Value::Bool(true))?;
    vm.set(id, "_alpha", Value::Number(100.0))?;
    if !name.is_empty() {
        vm.set(parent, name, Value::Object(id))?;
    }
    Ok(id)
}

/// Movie key of a loadClip url. Retail ScreenManagerBase::OpenScreen (sub_82D09140) passes
/// "source/screens/" + the template's path, which names the authoring file (`main/core_menu.swf`,
/// template table 0x8302F1A8); the loader opens the packed `.apt` of the same name.
fn screen_key(url: &str) -> String {
    let url = url.strip_suffix(".swf").unwrap_or(url);
    library_key(url)
}

impl Bindings<'_> {
    fn level_of(&self, id: usize) -> Option<usize> {
        self.state.levels.iter().position(|l| l.movie.instances.contains_key(&id))
    }
    fn clip_call(
        &mut self,
        vm: &mut Vm,
        object: usize,
        method: &str,
        args: &[Value],
    ) -> Result<Option<Value>, String> {
        match method {
            "createEmptyMovieClip" => {
                let name = args.first().map(Value::text).unwrap_or_default();
                let depth = args.get(1).map_or(0, crate::apt_vm::to_integer);
                let id = new_clip(vm, object, &name)?;
                self.state.dynamic.insert(id, (object, depth));
                return Ok(Some(Value::Object(id)));
            }
            "removeMovieClip" | "unloadMovie" => {
                self.unload_into(vm, object);
                if method == "removeMovieClip" {
                    if let Some((parent, _)) = self.state.dynamic.remove(&object) {
                        let name = vm.get(object, "_name").text();
                        if vm.get(parent, &name) == Value::Object(object) {
                            vm.objects[parent].fields.remove(&name);
                        }
                        vm.set(object, "_visible", Value::Bool(false))?;
                    }
                }
                return Ok(Some(Value::Undefined));
            }
            "getDepth" => {
                if let Some((_, depth)) = self.state.dynamic.get(&object) {
                    return Ok(Some(Value::Number(*depth as f64)));
                }
            }
            _ => {}
        }
        if let Some(level) = self.level_of(object) {
            if self.state.levels[level].movie.method(vm, object, method, args)? {
                return Ok(Some(Value::Undefined));
            }
        }
        Ok(None)
    }
    /// Removes a movie loaded into `target`, if any (unload queued like a removal).
    fn unload_into(&mut self, vm: &mut Vm, target: usize) {
        if let Some(i) = self.state.levels.iter().position(|l| l.movie.root == target) {
            let mut level = self.state.levels.remove(i);
            level.movie.unload(vm);
            // Its queued unload handlers still run (retail sub_82E55DA8 queues them).
            level.movie.instances.clear();
            let pending: Vec<_> = std::iter::from_fn(|| level.movie.next_action()).collect();
            if !pending.is_empty() {
                level.movie.pending = pending.into();
                self.state.levels.push(Level { key: format!("{}#unloading", level.key), ..level });
            }
        }
    }
}

impl Host for Bindings<'_> {
    fn trace(&mut self, message: &str) {
        self.state.traces.push(message.into());
    }
    fn call(
        &mut self,
        vm: &mut Vm,
        object: usize,
        method: &str,
        args: Vec<Value>,
    ) -> Result<Value, String> {
        let kind = vm.objects.get(object).map(|o| o.kind.clone());
        if let Some(ObjectKind::Native(name)) = &kind {
            if self.state.native_names.contains(name) {
                return self.natives.call(vm, name, method, args);
            }
            if name.starts_with("movie:") {
                if let Some(v) = self.clip_call(vm, object, method, &args)? {
                    return Ok(v);
                }
            }
        }
        if vm.objects.get(object).and_then(|o| o.prototype) == Some(self.state.loader_prototype) {
            match method {
                "addListener" | "removeListener" => {
                    let listeners = match vm.get(object, "_listeners") {
                        Value::Object(id) => id,
                        _ => {
                            let id = vm.array(vec![])?;
                            vm.set(object, "_listeners", Value::Object(id))?;
                            id
                        }
                    };
                    let n = vm.get(listeners, "length").number() as usize;
                    let mut items: Vec<Value> =
                        (0..n).map(|i| vm.get(listeners, &i.to_string())).collect();
                    let listener = args.first().cloned().unwrap_or_default();
                    items.retain(|v| *v != listener);
                    if method == "addListener" {
                        items.push(listener);
                    }
                    let id = vm.array(items)?;
                    vm.set(object, "_listeners", Value::Object(id))?;
                    return Ok(Value::Bool(true));
                }
                "loadClip" => {
                    let (Some(url), Some(Value::Object(target))) = (args.first(), args.get(1))
                    else {
                        return Ok(Value::Bool(false));
                    };
                    // Retail loads asynchronously (load queue sub_82E7E9F8); the movie appears
                    // on a later update, after the calling script finished.
                    self.state.loads.push_back(Load {
                        key: screen_key(&url.text()),
                        target: *target,
                        loader: object,
                    });
                    return Ok(Value::Bool(true));
                }
                "unloadClip" => {
                    if let Some(Value::Object(target)) = args.first() {
                        self.unload_into(vm, *target);
                    }
                    return Ok(Value::Bool(true));
                }
                _ => {}
            }
        }
        if object == self.state.object_class && method == "registerClass" {
            // Object.registerClass(linkage, class): clips created from that linkage name
            // later get the class (menu_picker, tabs, highlight_* register this way).
            if let (Some(name), Some(Value::Object(class))) = (args.first(), args.get(1)) {
                self.state.classes.insert(name.text(), *class);
                return Ok(Value::Bool(true));
            }
            return Ok(Value::Bool(false));
        }
        if object == vm.global && method == "ASSetPropFlags" {
            return Ok(Value::Undefined);
        }
        let owner = match &kind {
            Some(ObjectKind::Native(name)) => name.clone(),
            _ if object == vm.global => "_global".into(),
            _ => "object".into(),
        };
        self.state
            .unhandled
            .insert(format!("{owner}.{method} (at {:x})", vm.last_instruction.1));
        Ok(Value::Undefined)
    }
}

impl Stage {
    pub fn new(natives: &dyn StageNatives) -> Result<Self, String> {
        let mut vm = Vm::new();
        let mut native_names = BTreeSet::new();
        let mut loader_prototype = 0;
        let mut object_class = 0;
        for name in ["MovieClip", "MovieClipLoader", "Object", "Math", "Array", "String"] {
            let object = vm.object(ObjectKind::Plain);
            let prototype = vm.object(ObjectKind::Plain);
            vm.set(object, "prototype", Value::Object(prototype))?;
            vm.set(vm.global, name, Value::Object(object))?;
            if name == "MovieClipLoader" {
                loader_prototype = prototype;
            }
            if name == "Object" {
                object_class = object;
            }
        }
        for name in natives.objects() {
            let object = vm.object(ObjectKind::Native(name.clone()));
            vm.set(vm.global, &name, Value::Object(object))?;
            native_names.insert(name);
        }
        Ok(Self {
            vm,
            state: State {
                levels: Vec::new(),
                loads: VecDeque::new(),
                dynamic: BTreeMap::new(),
                loader_prototype,
                object_class,
                classes: BTreeMap::new(),
                libraries: BTreeSet::new(),
                native_names,
                unhandled: BTreeSet::new(),
                traces: Vec::new(),
            },
        })
    }
    /// A script-less empty clip under `parent` (what retail C++ makes to hold the screen manager movie).
    pub fn create_clip(&mut self, parent: usize, name: &str) -> Result<usize, String> {
        let id = new_clip(&mut self.vm, parent, name)?;
        self.state.dynamic.insert(id, (parent, 0));
        Ok(id)
    }
    pub fn levels(&self) -> &[Level] {
        &self.state.levels
    }
    pub fn unhandled(&self) -> &BTreeSet<String> {
        &self.state.unhandled
    }
    pub fn take_traces(&mut self) -> Vec<String> {
        std::mem::take(&mut self.state.traces)
    }
    /// The root movie (retail FE_root): loaded at the stage root, `_level0`.
    pub fn load_root(
        &mut self,
        key: &str,
        source: &dyn MovieSource,
        natives: &mut dyn StageNatives,
    ) -> Result<usize, String> {
        let movie = source.movie(key).ok_or_else(|| format!("APT stage: no movie {key}"))?;
        let root = self.start(key, movie, None, source, natives)?;
        self.vm.set(self.vm.global, "_level0", Value::Object(root))?;
        Ok(root)
    }
    /// Loads `key` into the clip `target` now (what a queued loadClip does on the next update).
    pub fn load_into(
        &mut self,
        key: &str,
        target: usize,
        source: &dyn MovieSource,
        natives: &mut dyn StageNatives,
    ) -> Result<usize, String> {
        let movie = source.movie(key).ok_or_else(|| format!("APT stage: no movie {key}"))?;
        self.start(key, movie, Some(target), source, natives)
    }
    fn start(
        &mut self,
        key: &str,
        mut movie: Movie,
        into: Option<usize>,
        source: &dyn MovieSource,
        natives: &mut dyn StageNatives,
    ) -> Result<usize, String> {
        self.vm.begin_update();
        // Retail loads every imported library movie before linking (sub_82E7E8C0); a library's
        // root init actions (its class definitions and registerClass calls) run then.
        for library in movie.import_files.clone() {
            self.init_library(&library, source, natives, 0)?;
        }
        // Init actions (class definitions such as _global.screenmanager) run once, before the
        // first frame, like the HUD runtime does for its movie.
        let initial: Vec<_> = movie
            .characters
            .values()
            .flat_map(|c| &c.frames)
            .flat_map(|f| &f.controls)
            .filter(|c| c.type_name == "do_init_action")
            .map(|c| c.actions_offset)
            .collect();
        for offset in initial {
            let code = movie
                .actions
                .get(&offset.to_string())
                .ok_or("APT stage: missing init action block")?
                .clone();
            let mut host = Bindings { state: &mut self.state, natives };
            self.vm.run(&code, &mut host).map_err(|e| {
                let (op, at) = self.vm.last_instruction;
                format!("{key} init action {offset}: {e} (op {op:02x} at {at:x})")
            })?;
        }
        if let Some(target) = into {
            self.unload_target(target, natives);
        }
        movie.classes = movie
            .linkage
            .iter()
            .filter_map(|(name, character)| Some((*character, *self.state.classes.get(name)?)))
            .collect();
        movie.initialize_into(&mut self.vm, into)?;
        let root = movie.root;
        // Retail load completion (sub_82E7EF28) gives a loaded root clip flag 0x80, not the playing flag 0x40
        // (set only by play(), action 0x06 82E6BCC8); the tick (82E5B640) advances 0x80 clips only when the APT
        // config byte [0x83037510+64] is set, which Skate 3's init leaves 0. So a loaded root runs its frame-0
        // actions once and stays there until its script calls play(); its child sprites play as usual.
        if let Some(instance) = movie.instances.get_mut(&root) {
            instance.playing = false;
        }
        self.state.levels.push(Level { key: key.into(), movie, elapsed: 0.0 });
        self.drain(natives)?;
        Ok(root)
    }
    fn init_library(
        &mut self,
        key: &str,
        source: &dyn MovieSource,
        natives: &mut dyn StageNatives,
        depth: usize,
    ) -> Result<(), String> {
        if depth > 16 || !self.state.libraries.insert(key.to_string()) {
            return Ok(());
        }
        let Some(library) = source.movie(key) else {
            // Retail leaves the import slot null for a missing source (apt_imports).
            return Ok(());
        };
        for inner in &library.import_files {
            self.init_library(inner, source, natives, depth + 1)?;
        }
        let Some(root) = library.characters.get(&0) else {
            return Ok(());
        };
        let offsets: Vec<_> = root
            .frames
            .iter()
            .flat_map(|f| &f.controls)
            .filter(|c| c.type_name == "do_init_action")
            .map(|c| c.actions_offset)
            .collect();
        for offset in offsets {
            let Some(code) = library.actions.get(&offset.to_string()) else {
                continue;
            };
            let mut host = Bindings { state: &mut self.state, natives };
            self.vm.run(code, &mut host).map_err(|e| {
                let (op, at) = self.vm.last_instruction;
                format!("{key} library init action {offset}: {e} (op {op:02x} at {at:x})")
            })?;
        }
        Ok(())
    }
    fn unload_target(&mut self, target: usize, natives: &mut dyn StageNatives) {
        let mut host = Bindings { state: &mut self.state, natives };
        host.unload_into(&mut self.vm, target);
    }
    /// Runs queued actions of every movie until all queues are empty.
    pub fn drain(&mut self, natives: &mut dyn StageNatives) -> Result<(), String> {
        for _ in 0..ACTION_LIMIT {
            let next = self.state.levels.iter_mut().find_map(|l| {
                let p = l.movie.next_action()?;
                Some((p, l.movie.actions.get(&p.offset.to_string()).cloned()))
            });
            let Some((Pending { object, offset, event }, code)) = next else {
                self.state.levels.retain(|l| !l.key.ends_with("#unloading"));
                return Ok(());
            };
            if event == CLASS_CONSTRUCTOR {
                let class = self
                    .state
                    .levels
                    .iter()
                    .find_map(|l| l.movie.classes.get(&l.movie.instances.get(&object)?.character))
                    .copied();
                if let Some(class) = class {
                    let mut host = Bindings { state: &mut self.state, natives };
                    self.vm.call_function(class, object, vec![], &mut host)?;
                }
                continue;
            }
            let code = code.ok_or("APT stage: missing action block")?;
            let mut host = Bindings { state: &mut self.state, natives };
            self.vm.run_on(object, &code, &mut host).map_err(|e| {
                let (op, at) = self.vm.last_instruction;
                format!("action {offset}: {e} (op {op:02x} at {at:x})")
            })?;
        }
        Err("APT stage action limit".into())
    }
    /// Script call into the stage (what retail C++ does with `_global.ScreenManager.OpenScreen`
    /// and friends): resolves the dotted path from `_global`, calls it, then runs what it queued.
    pub fn call_path(
        &mut self,
        path: &str,
        args: Vec<Value>,
        natives: &mut dyn StageNatives,
    ) -> Result<Value, String> {
        self.vm.begin_update();
        let path = path.strip_prefix("_global.").unwrap_or(path);
        let (owner, method) = match path.rsplit_once('.') {
            Some((owner, method)) => (owner, method),
            None => ("", path),
        };
        let mut object = self.vm.global;
        for part in owner.split('.').filter(|p| !p.is_empty()) {
            match self.vm.get(object, part) {
                Value::Object(id) => object = id,
                _ => return Err(format!("APT stage: no {part} in {path}")),
            }
        }
        let mut host = Bindings { state: &mut self.state, natives };
        let value = self.vm.call_method(object, method, args, &mut host)?;
        self.drain(natives)?;
        Ok(value)
    }
    /// One engine update of `dt_ms`: queued loads complete, then every movie advances by the
    /// frames its own rate gives.
    pub fn update(
        &mut self,
        dt_ms: f32,
        source: &dyn MovieSource,
        natives: &mut dyn StageNatives,
    ) -> Result<(), String> {
        while let Some(load) = self.state.loads.pop_front() {
            if !self.vm.objects.get(load.target).is_some_and(|o| {
                matches!(&o.kind, ObjectKind::Native(n) if n.starts_with("movie:"))
            }) {
                continue;
            }
            let Some(movie) = source.movie(&load.key) else {
                self.notify(load.loader, "onLoadError", vec![Value::Object(load.target)], natives)?;
                continue;
            };
            self.notify(load.loader, "onLoadStart", vec![Value::Object(load.target)], natives)?;
            self.start(&load.key, movie, Some(load.target), source, natives)?;
            self.notify(load.loader, "onLoadComplete", vec![Value::Object(load.target)], natives)?;
            self.notify(load.loader, "onLoadInit", vec![Value::Object(load.target)], natives)?;
        }
        self.vm.begin_update();
        let count = self.state.levels.len();
        for i in 0..count {
            let Some(level) = self.state.levels.get_mut(i) else { break };
            let rate = level.movie.milliseconds_per_frame.unwrap_or(DEFAULT_MS_PER_FRAME);
            level.elapsed += dt_ms.max(0.0);
            let mut frames = 0;
            while level.elapsed >= rate && frames < MAX_FRAMES_PER_UPDATE {
                level.elapsed -= rate;
                frames += 1;
            }
            if frames == MAX_FRAMES_PER_UPDATE {
                level.elapsed = level.elapsed.min(rate);
            }
            for _ in 0..frames {
                let Some(level) = self.state.levels.get_mut(i) else { break };
                level.movie.advance(&mut self.vm)?;
                self.drain(natives)?;
            }
        }
        Ok(())
    }
    /// Placement of the clip a level's root is loaded into, from its ancestors (each an instance of another
    /// level or a script-made clip): matrix, alpha factor, and whether every ancestor is visible.
    fn parent_of(&self, root: usize) -> ([f32; 6], f32, bool) {
        let mut matrix = crate::apt_scene::IDENTITY;
        let mut alpha = 1.0;
        let mut current = self.vm.get(root, "_parent");
        for _ in 0..64 {
            let Value::Object(id) = current else { break };
            if !self.vm.get(id, "_visible").truth() {
                return (matrix, alpha, false);
            }
            let mut m = self
                .state
                .levels
                .iter()
                .find_map(|l| l.movie.instances.get(&id)?.placement.as_ref().map(|p| p.matrix))
                .unwrap_or(crate::apt_scene::IDENTITY);
            m[4] = self.vm.get(id, "_x").number() as f32;
            m[5] = self.vm.get(id, "_y").number() as f32;
            matrix = crate::apt_scene::compose(m, matrix);
            alpha *= (self.vm.get(id, "_alpha").number() as f32 / 100.).clamp(0., 1.);
            current = self.vm.get(id, "_parent");
        }
        (matrix, alpha, true)
    }
    /// Draws every level in load order (FE_root, the screen manager, then screens in the order they opened),
    /// each under the clip it was loaded into. `scene` gives a level's shapes and textures.
    /// NOT RETAIL YET (order): retail draws one display tree by depth; screens sit at depth 600 + template
    /// index under the screen manager, so load order matches it for the screens opened so far.
    pub fn draw<'s, S: crate::apt_scene::ShapeSource>(
        &'s self,
        scene: impl Fn(&'s str, &'s Movie) -> S,
    ) -> Result<Vec<crate::apt_scene::Draw>, String> {
        let mut out = Vec::new();
        for level in &self.state.levels {
            if level.key.ends_with("#unloading") {
                continue;
            }
            let (matrix, alpha, visible) = self.parent_of(level.movie.root);
            if !visible || alpha <= 0.0 {
                continue;
            }
            let shapes = scene(&level.key, &level.movie);
            out.extend(crate::apt_scene::draw_from(
                &level.movie,
                &self.vm,
                &shapes,
                matrix,
                [1., 1., 1., alpha],
                [0.; 4],
            )?);
        }
        Ok(out)
    }
    fn notify(
        &mut self,
        loader: usize,
        event: &str,
        args: Vec<Value>,
        natives: &mut dyn StageNatives,
    ) -> Result<(), String> {
        let Value::Object(list) = self.vm.get(loader, "_listeners") else {
            return Ok(());
        };
        let n = self.vm.get(list, "length").number().max(0.0) as usize;
        for i in 0..n {
            if let Value::Object(listener) = self.vm.get(list, &i.to_string()) {
                if let Value::Object(_) = self.vm.get(listener, event) {
                    let mut host = Bindings { state: &mut self.state, natives };
                    self.vm.call_method(listener, event, args.clone(), &mut host)?;
                }
            }
        }
        self.drain(natives)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_string_and_array_builtins() {
        let mut vm = Vm::new();
        let a = vm.array(vec![Value::Number(1.0), Value::Number(2.0)]).unwrap();
        struct Stub;
        impl Host for Stub {
            fn call(&mut self, _: &mut Vm, _: usize, m: &str, _: Vec<Value>) -> Result<Value, String> {
                Err(format!("host {m}"))
            }
        }
        assert_eq!(vm.call_method(a, "push", vec![Value::Number(3.0)], &mut Stub), Ok(Value::Number(3.0)));
        assert_eq!(vm.call_method(a, "join", vec![Value::Text("-".into())], &mut Stub), Ok(Value::Text("1-2-3".into())));
        let Ok(Value::Object(removed)) = vm.call_method(a, "splice", vec![Value::Number(0.0), Value::Number(1.0)], &mut Stub) else {
            panic!()
        };
        assert_eq!(vm.get(removed, "0"), Value::Number(1.0));
        assert_eq!(vm.get(a, "length"), Value::Number(2.0));
        assert_eq!(vm.call_method(a, "pop", vec![], &mut Stub), Ok(Value::Number(3.0)));
        assert!(vm.call_method(a, "unknown", vec![], &mut Stub).is_err());
    }

    /// The real retail screen manager + core_menu from the exported set (skipped without it): the
    /// screen manager's class registers, OpenScreen with retail's CrossBar arguments creates a
    /// clip and loads the screen into it on the next update, and the screen reports
    /// OnScreenLoaded / OnScreenIntroComplete like retail (AS id 600 + template index).
    #[test]
    fn retail_screen_manager_opens_a_screen() {
        retail_screen_manager(false);
    }

    /// CloseScreen after many updates: the screen manager's 2-frame root must not loop (loaded roots do not
    /// play, doc 31 "Milestone 5a"), or `new screenmanager(this)` would reset mMovieClipList.
    #[test]
    fn retail_screen_manager_closes_a_screen() {
        retail_screen_manager(true);
    }

    fn retail_screen_manager(close: bool) {
        let root = std::env::var("SKATE3_MENU_MOVIES")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| crate::apt_imports::main_checkout().join("assets/private/menu-movies"));
        if !root.join("manifest.json").is_file() {
            eprintln!("skipped: no exported menu movies in {}", root.display());
            return;
        }
        let set = crate::retail_menu_movies::RetailMenuMovies::read(&root).unwrap();
        let movies = set.movies.clone();
        let registry = crate::crossbar_host::test_registry();
        let context = skate_core::menus::MenuContext::new("FreePlay");
        let language = movies
            .get("source/screens/main/core_menu")
            .map(|m| m.text_assets.language.clone())
            .unwrap_or_default();
        let mut game = crate::crossbar_host::FrontEnd::new(&registry, &context, &language);
        let mut stage = Stage::new(&game).unwrap();
        let fe = stage.load_root("source/screens/main/FE_root", &movies, &mut game).unwrap();
        // FE_root's own script (retail: "FE_ROOT SETUP!").
        assert!(stage.take_traces().iter().any(|t| t.contains("FE_ROOT")));
        let holder = new_clip(&mut stage.vm, fe, "screenmanager_mc").unwrap();
        stage.load_into("source/helper/screenmanager", holder, &movies, &mut game).unwrap();
        let Value::Object(manager) = stage.vm.get(stage.vm.global, "ScreenManager") else {
            panic!("ScreenManager not registered; unhandled: {:?}", stage.unhandled());
        };
        assert_eq!(stage.vm.get(manager, "currentDepth"), Value::Number(600.0));
        stage
            .call_path("_global.ScreenManager.RegisterTemplate", vec![Value::Text("CrossBar".into())], &mut game)
            .unwrap();
        stage
            .call_path(
                "_global.ScreenManager.OpenScreen",
                vec![
                    // Retail arguments for template 69 "CrossBar" (sub_82D09140).
                    Value::Text("source/screens/main/core_menu.swf".into()),
                    Value::Text("screen".into()),
                    Value::Text("0".into()),
                    Value::Text("CrossBar".into()),
                ],
                &mut game,
            )
            .unwrap();
        assert!(stage.levels().iter().all(|l| l.key != "source/screens/main/core_menu"));
        stage.update(16.0, &movies, &mut game).unwrap();
        for _ in 0..60 {
            stage.update(16.0, &movies, &mut game).unwrap();
        }
        eprintln!("levels: {:?}", stage.levels().iter().map(|l| &l.key).collect::<Vec<_>>());
        eprintln!("game calls: {:?}", game.calls);
        eprintln!("unhandled: {:?}", stage.unhandled());
        assert!(stage.levels().iter().any(|l| l.key == "source/screens/main/core_menu"));
        // The open pause menu draws, and every texture it names resolves in the exported set.
        let draws = stage.draw(|key, movie| set.scene(key, movie)).unwrap();
        let missing: Vec<_> = draws
            .iter()
            .filter(|d| d.fill.textured() && set.image(&d.texture).is_none())
            .map(|d| d.texture.clone())
            .collect();
        eprintln!("draws: {} (textured {})", draws.len(), draws.iter().filter(|d| d.fill.textured()).count());
        if std::env::var("SKATE3_DUMP_DRAWS").is_ok() {
            // Retail crossbar IntroComplete posts SetState(InitialiseMenu = 2) (8261B9E8).
            stage
                .call_path("_global.ScreenManager.SetScreenState", vec![Value::Text("600".into()), Value::Text("2".into())], &mut game)
                .unwrap();
            for _ in 0..440 {
                stage.update(16.0, &movies, &mut game).unwrap();
            }
            for l in stage.levels() {
                let r = &l.movie.instances[&l.movie.root];
                eprintln!("LEVEL {} frame {} playing {} root _x {} _y {} vis {}", l.key, r.frame, r.playing,
                    stage.vm.get(l.movie.root, "_x").text(), stage.vm.get(l.movie.root, "_y").text(), stage.vm.get(l.movie.root, "_visible").text());
                fn walk(l: &Level, vm: &Vm, id: usize, depth: usize) {
                    let i = &l.movie.instances[&id];
                    let c = &l.movie.characters[&i.character];
                    eprintln!("{:indent$}- {} {} char {} f{}/{} col {:?} vis {} a {} x {} y {} text {:?}", "", c.type_name,
                        i.placement.as_ref().map_or("", |p| p.name.as_str()), i.character, i.frame, c.frames.len(), i.placement.as_ref().map(|p| p.color),
                        vm.get(id, "_visible").text(), vm.get(id, "_alpha").text(), vm.get(id, "_x").text(), vm.get(id, "_y").text(),
                        vm.get(id, "_displayText").text(), indent = depth * 2);
                    if depth < 4 {
                        for child in i.children.values() {
                            walk(l, vm, *child, depth + 1);
                        }
                    }
                }
                if l.key.ends_with("core_menu") {
                    for (_, child) in &r.children {
                        let name = l.movie.instances[child].placement.as_ref().map_or("", |p| p.name.as_str());
                        if ["cat0", "mTitle", "mColumnAnim"].contains(&name) {
                            walk(l, &stage.vm, *child, 1);
                        }
                    }
                }
                for (depth, child) in &r.children {
                    let i = &l.movie.instances[child];
                    eprintln!("  child d{} {} char {} frame {}/{} playing {} _x {} _y {} vis {}", depth,
                        i.placement.as_ref().map_or("", |p| p.name.as_str()), i.character, i.frame,
                        l.movie.characters[&i.character].frames.len(), i.playing,
                        stage.vm.get(*child, "_x").text(), stage.vm.get(*child, "_y").text(), stage.vm.get(*child, "_visible").text());
                }
            }
            for d in stage.draw(|key, movie| set.scene(key, movie)).unwrap() {
                let xs = d.vertices.iter().map(|v| v.position[0]);
                let ys = d.vertices.iter().map(|v| v.position[1]);
                let (x0, x1) = xs.fold((f32::MAX, f32::MIN), |(a, b), x| (a.min(x), b.max(x)));
                let (y0, y1) = ys.fold((f32::MAX, f32::MIN), |(a, b), y| (a.min(y), b.max(y)));
                eprintln!("DRAW {:?} {:.0},{:.0}..{:.0},{:.0} mul {:?} add {:?} {}", d.fill, x0, y0, x1, y1, d.multiply, d.add,
                    d.texture.rsplit('/').next().unwrap_or(""));
            }
        }
        assert!(!draws.is_empty());
        assert!(missing.is_empty(), "missing textures {missing:?}");
        if !close {
            return;
        }
        // Retail CloseScreen (sub_82D09298) passes the AS id as "%d"; the screen plays its outro
        // and reports OutroComplete.
        game.calls.clear();
        stage
            .call_path("_global.ScreenManager.CloseScreen", vec![Value::Text("600".into())], &mut game)
            .unwrap();
        for _ in 0..60 {
            stage.update(16.0, &movies, &mut game).unwrap();
        }
        eprintln!("after close: {:?}", game.calls);
        assert!(game.calls.iter().any(|c| c == "Game.OnScreenOutroComplete(600)"), "{:?}", game.calls);
    }
}
