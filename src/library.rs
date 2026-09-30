use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

/// Tracks which libraries a running program has imported, and resolves
/// custom packages that were downloaded with `akk install <name>`.
///
/// Three kinds of libraries exist:
///   1. Built-in libraries (အချိန်, ကျပန်း, request) -- compiled directly
///      into the `akk` binary. The interpreter recognizes these names
///      itself and just calls `mark_loaded` on them.
///   2. Downloaded packages -- plain Akkhara source files that live under
///      `<libraries_dir>/<name>/main/`. `find_dynamic_source` reads the
///      entry file named in `<name>/index.json`'s `entry` field
///      (defaulting to `main/main.akk` when there's no manifest, or no
///      `entry` field in it) so the interpreter can lex/parse/run it like
///      any other Akkhara program, which registers its functions/classes
///      globally.
///   3. Plain scripts -- a `<name>.akk` file sitting next to the running
///      program (or in the current working directory). `find_script_source`
///      reads it, so `နည်းပညာများ Tools ကို အသုံးပြုပါ။` can pull in a
///      neighboring `Tools.akk` without a separate `akk install` step.
pub struct LibraryLoader {
    loaded: HashMap<String, ()>,
    /// Names whose source is currently being run. A script that imports
    /// itself (directly, or through a cycle of other scripts) would
    /// otherwise recurse forever, so `begin_loading` reports the repeat.
    loading: HashSet<String>,
    libraries_dir: PathBuf,
    /// Extra folders searched for a plain `<name>.akk` script, in order.
    /// Populated with the folder of the program being run, so a script can
    /// import a sibling script.
    script_dirs: Vec<PathBuf>,
}

impl LibraryLoader {
    pub fn new(libraries_dir: PathBuf) -> Self {
        LibraryLoader {
            loaded: HashMap::new(),
            loading: HashSet::new(),
            libraries_dir,
            script_dirs: Vec::new(),
        }
    }

    pub fn mark_loaded(&mut self, name: &str) {
        self.loaded.insert(name.to_string(), ());
    }

    pub fn is_loaded(&self, name: &str) -> bool {
        self.loaded.contains_key(name)
    }

    /// Registers a folder to search for `<name>.akk` scripts, after any
    /// folders added before it and before the libraries/ folder itself.
    pub fn add_script_dir(&mut self, dir: PathBuf) {
        if !self.script_dirs.contains(&dir) {
            self.script_dirs.push(dir);
        }
    }

    /// Marks `name` as being loaded and returns `true`; returns `false` when
    /// it is already mid-load, which means the imports form a cycle.
    /// Always pair a `true` result with `end_loading`.
    pub fn begin_loading(&mut self, name: &str) -> bool {
        self.loading.insert(name.to_string())
    }

    pub fn end_loading(&mut self, name: &str) {
        self.loading.remove(name);
    }

    /// Reads a downloaded package's entry source file, if one exists.
    /// Consults `<libraries_dir>/<name>/index.json`'s `entry` field for
    /// the path to the entry file (relative to the package folder),
    /// defaulting to `main/main.akk` when there's no manifest or no
    /// `entry` field. Returns `None` if there's no such package, so the
    /// caller can fall back to a "library not found" error.
    pub fn find_dynamic_source(&self, name: &str) -> Option<String> {
        let pkg_dir = self.libraries_dir.join(name);
        let entry = fs::read_to_string(pkg_dir.join("index.json"))
            .ok()
            .and_then(|manifest| crate::extract_json_string_field(&manifest, "entry"))
            .unwrap_or_else(|| "main/main.akk".to_string());
        fs::read_to_string(pkg_dir.join(entry)).ok()
    }

    /// Reads a plain `<name>.akk` script, looking in the program's own
    /// folder (and the current working directory) first, then next to the
    /// akk binary. A `name` that already ends in `.akk` is used as-is.
    /// Returns `None` when no such script exists.
    pub fn find_script_source(&self, name: &str) -> Option<String> {
        let file = if name.ends_with(".akk") {
            name.to_string()
        } else {
            format!("{}.akk", name)
        };
        for dir in self.script_dirs.iter().chain(std::iter::once(&self.libraries_dir)) {
            if let Ok(src) = fs::read_to_string(dir.join(&file)) {
                return Some(src);
            }
        }
        None
    }
}
