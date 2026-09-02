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
///      `<libraries_dir>/<name>/main.akk`. `find_dynamic_source` reads one
///      of these so the interpreter can lex/parse/run it like any other
///      Akkhara program, which registers its functions/classes globally.
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

    /// Reads `<libraries_dir>/<name>/main.akk` if a downloaded package by
    /// that name exists. Returns `None` if there's no such package, so the
    /// caller can fall back to a "library not found" error.
    pub fn find_dynamic_source(&self, name: &str) -> Option<String> {
        let path = self.libraries_dir.join(name).join("main.akk");
        fs::read_to_string(path).ok()
    }
}
