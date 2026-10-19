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
    /// `[dependencies]` entries that name a `path` (from `akk.toml`), as
    /// `(library name, folder or .akk file)`. They are searched before any
    /// sibling script or downloaded package, so a project's own copy of a
    /// library is the one it imports.
    dependencies: Vec<(String, PathBuf)>,
}

/// What a manifest dependency resolved to: a built-in library (no source
/// file -- its code is compiled into `akk`), or the source file to run.
pub enum DependencyResolution {
    Builtin,
    Source(PathBuf),
}

/// The names the interpreter recognizes as libraries compiled into this
/// binary, in the spelling(s) each answers to. Every name in a group marks
/// the whole group loaded, so `server` and `ဆာဗာ` are one library.
pub fn builtin_names(name: &str) -> Option<&'static [&'static str]> {
    const RANDOM: &[&str] = &["ကျပန်း"];
    const TIME: &[&str] = &["အချိန်"];
    const REQUEST: &[&str] = &["request"];
    const APP: &[&str] = &["App"];
    const SERVER: &[&str] = &["server", "ဆာဗာ"];
    const JSON: &[&str] = &["json", "ဂျေဆန်"];
    const FILE: &[&str] = &["file", "ဖိုင်"];

    match name {
        "ကျပန်း" => Some(RANDOM),
        "အချိန်" => Some(TIME),
        "request" => Some(REQUEST),
        "App" => Some(APP),
        "server" | "ဆာဗာ" => Some(SERVER),
        "json" | "ဂျေဆန်" => Some(JSON),
        "file" | "ဖိုင်" => Some(FILE),
        _ => None,
    }
}

/// Does `name` name a library compiled into this binary?
pub fn is_builtin_library(name: &str) -> bool {
    builtin_names(name).is_some()
}

impl LibraryLoader {
    pub fn new(libraries_dir: PathBuf) -> Self {
        LibraryLoader {
            loaded: HashMap::new(),
            loading: HashSet::new(),
            libraries_dir,
            script_dirs: Vec::new(),
            dependencies: Vec::new(),
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

    /// Registers a `[dependencies]` entry written as `{ path = "..." }`.
    /// `path` may name a folder (whose `main.akk` or `<name>.akk` is the
    /// entry) or a `.akk` file directly.
    pub fn add_dependency(&mut self, name: &str, path: PathBuf) {
        if !self.dependencies.iter().any(|(n, _)| n == name) {
            self.dependencies.push((name.to_string(), path));
        }
    }

    /// The source file a `{ path = ... }` dependency names, when it has one.
    fn dependency_path(&self, name: &str) -> Option<PathBuf> {
        let (_, path) = self.dependencies.iter().find(|(n, _)| n == name)?;
        if path.is_file() {
            return Some(path.clone());
        }
        let main = path.join("main.akk");
        if main.is_file() {
            return Some(main);
        }
        let named = path.join(format!("{}.akk", name));
        named.is_file().then_some(named)
    }

    /// Resolve a `[dependencies]` name in the order `akk.toml` documents:
    /// a `path` entry, then a sibling `<name>.akk`, then a downloaded package
    /// under `libraries/<name>/`, then a built-in library.
    pub fn resolve_dependency(&self, name: &str) -> Option<DependencyResolution> {
        if let Some(path) = self.dependency_path(name) {
            return Some(DependencyResolution::Source(path));
        }
        if let Some(path) = self.script_path(name) {
            return Some(DependencyResolution::Source(path));
        }
        if let Some(path) = self.dynamic_source_path(name) {
            return Some(DependencyResolution::Source(path));
        }
        is_builtin_library(name).then_some(DependencyResolution::Builtin)
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

    /// Reads a downloaded package's entry file, together with the path it
    /// was read from. Consults `<libraries_dir>/<name>/index.json`'s
    /// `entry` field for the path to the entry file (relative to the
    /// package folder), defaulting to `main/main.akk` when there's no
    /// manifest or no `entry` field. Returns `None` if there's no such
    /// package, so the caller can fall back to a "library not found" error.
    pub fn find_dynamic_source(&self, name: &str) -> Option<String> {
        let path = self
            .dependency_path(name)
            .or_else(|| self.dynamic_source_path(name))?;
        fs::read_to_string(path).ok()
    }

    /// Reads a plain `<name>.akk` script, looking in the program's own
    /// folder (and the current working directory) first, then next to the
    /// akk binary. A `name` that already ends in `.akk` is used as-is.
    /// Returns `None` when no such script exists.
    pub fn find_script_source(&self, name: &str) -> Option<String> {
        let path = self.dependency_path(name).or_else(|| self.script_path(name))?;
        fs::read_to_string(path).ok()
    }

    /// Finds a library's Akkhara source and the file it came from, trying a
    /// downloaded package first and then a plain `<name>.akk` script -- the
    /// same order `import_library` uses. `None` for a built-in library (its
    /// code lives in the binary, not in a file) and for an unknown name.
    ///
    /// The static warning pass uses this to check an imported script's own
    /// source; a run uses `find_dynamic_source`/`find_script_source` and does
    /// not need the path.
    pub fn find_source(&self, name: &str) -> Option<(PathBuf, String)> {
        let path = self
            .dependency_path(name)
            .or_else(|| self.dynamic_source_path(name))
            .or_else(|| self.script_path(name))?;
        fs::read_to_string(&path).ok().map(|src| (path, src))
    }

    /// The file a downloaded package's entry lives in, whether or not it can
    /// be read.
    fn dynamic_source_path(&self, name: &str) -> Option<PathBuf> {
        let pkg_dir = self.libraries_dir.join(name);
        let entry = fs::read_to_string(pkg_dir.join("index.json"))
            .ok()
            .and_then(|manifest| crate::extract_json_string_field(&manifest, "entry"))
            .unwrap_or_else(|| "main/main.akk".to_string());
        let path = pkg_dir.join(entry);
        path.is_file().then_some(path)
    }

    /// The first `<name>.akk` file found in the script folders, whether or not
    /// it can be read.
    fn script_path(&self, name: &str) -> Option<PathBuf> {
        let file = if name.ends_with(".akk") {
            name.to_string()
        } else {
            format!("{}.akk", name)
        };
        self.script_dirs
            .iter()
            .chain(std::iter::once(&self.libraries_dir))
            .map(|dir| dir.join(&file))
            .find(|path| path.is_file())
    }
}

#[cfg(test)]
mod tests {
    use super::LibraryLoader;
    use std::fs;
    use std::path::PathBuf;

    /// A fresh empty folder for one test, under the system temp folder.
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("akk-loader-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp folder");
        dir
    }

    #[test]
    fn find_source_reads_a_packages_entry_file() {
        // A downloaded package: `<libraries>/<name>/index.json` says which file
        // is the entry, and that file is what the warning pass reads.
        let dir = temp_dir("pkg");
        fs::create_dir_all(dir.join("Pkg").join("lib")).expect("package folder");
        fs::write(
            dir.join("Pkg").join("index.json"),
            "{\"name\":\"Pkg\",\"entry\":\"lib/entry.akk\"}",
        )
        .expect("manifest");
        fs::write(
            dir.join("Pkg").join("lib").join("entry.akk"),
            "fn one() {\n    return 1;\n}\n",
        )
        .expect("entry");

        let loader = LibraryLoader::new(dir.clone());
        let (path, src) = loader.find_source("Pkg").expect("package source");
        assert_eq!(path.file_name().and_then(|n| n.to_str()), Some("entry.akk"));
        assert!(src.contains("fn one()"), "{}", src);
        // A built-in has no source file, so the pass has nothing to read.
        assert!(loader.find_source("ကျပန်း").is_none());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn find_source_searches_the_programs_folder_before_the_libraries_folder() {
        // The program's own folder is searched first, so a sibling
        // `<name>.akk` is the one a run imports -- and the one a warning names.
        let dir = temp_dir("script");
        let program_dir = dir.join("program");
        fs::create_dir_all(&program_dir).expect("program folder");
        fs::write(program_dir.join("Tools.akk"), "fn from_program() {\n}\n").expect("sibling");
        fs::write(dir.join("Tools.akk"), "fn from_libraries() {\n}\n").expect("libraries");

        let mut loader = LibraryLoader::new(dir.clone());
        loader.add_script_dir(program_dir.clone());
        let (path, src) = loader.find_source("Tools").expect("script source");
        assert_eq!(path, program_dir.join("Tools.akk"));
        assert!(src.contains("from_program"), "{}", src);

        // The `.akk` spelling names the same file.
        let named = loader.find_source("Tools.akk").expect("script by file name");
        assert_eq!(named.0, path);

        // Without the program folder, the libraries folder answers instead.
        let plain = LibraryLoader::new(dir.clone());
        let (fallback, _) = plain.find_source("Tools").expect("libraries script");
        assert_eq!(fallback, dir.join("Tools.akk"));

        assert!(loader.find_source("Missing").is_none());
        fs::remove_dir_all(&dir).ok();
    }
}
