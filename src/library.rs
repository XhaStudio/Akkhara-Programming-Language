use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Tracks which libraries a running program has imported, and resolves
/// custom packages that were downloaded with `akk install <name>`.
///
/// Two kinds of libraries exist:
///   1. Built-in libraries (အချိန်, ကျပန်း) -- compiled directly into the
///      `akk` binary. The interpreter recognizes these names itself and
///      just calls `mark_loaded` on them.
///   2. Downloaded packages -- plain Akkhara source files that live under
///      `<libraries_dir>/<name>/main/`. `find_dynamic_source` reads the
///      entry file named in `<name>/index.json`'s `entry` field
///      (defaulting to `main/main.akk` when there's no manifest, or no
///      `entry` field in it) so the interpreter can lex/parse/run it like
///      any other Akkhara program, which registers its functions/classes
///      globally.
pub struct LibraryLoader {
    loaded: HashMap<String, ()>,
    libraries_dir: PathBuf,
}

impl LibraryLoader {
    pub fn new(libraries_dir: PathBuf) -> Self {
        LibraryLoader {
            loaded: HashMap::new(),
            libraries_dir,
        }
    }

    pub fn mark_loaded(&mut self, name: &str) {
        self.loaded.insert(name.to_string(), ());
    }

    pub fn is_loaded(&self, name: &str) -> bool {
        self.loaded.contains_key(name)
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
}
