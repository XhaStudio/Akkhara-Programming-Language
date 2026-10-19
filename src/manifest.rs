//! `akk.toml` -- a project's manifest.
//!
//! A manifest lives at the root of a project and is found by `akk run` (and,
//! later, `akk build`) by looking in the current directory and then every
//! parent directory. It gives those commands an entry file, the run settings
//! the project wants, and the dependencies the program may import.
//!
//! ```toml
//! [package]
//! name = "hello"
//! version = "0.1.0"
//! entry = "main.akk"          # default: main.akk
//!
//! [build]
//! mode = "bundle"             # bundle | bytecode (default: bundle)
//! output = "hello"            # default: the package name
//! out_dir = "build"           # default: build
//! optimize = true             # default: true
//!
//! [build.bundle]
//! include = ["assets"]        # files/folders embedded in the bundle
//!
//! [run]
//! transcript = true           # write <file>.akop (default: true)
//! warnings = true             # show warnings (default: true)
//! timing = true               # print "Interpreted in ..." (default: true)
//!
//! [dependencies]
//! Tools = "*"
//! Local = { path = "vendor/local" }
//! Remote = { git = "https://example.com/remote.git", rev = "main" }
//!
//! [target.windows]
//! output = "hello.exe"
//! ```
//!
//! Every optional field has a default, so a manifest may be as small as an
//! empty file. A key this module does not know is a warning (`W008`), never
//! an error: a newer manifest can still be read by an older `akk`. A value
//! that cannot be used -- an unknown build mode, an entry file that is not
//! there, a dependency nothing can supply -- is an error, with its own code:
//!
//! | code | meaning |
//! |---|---|
//! | `E146` | `akk.toml` cannot be read or parsed as TOML |
//! | `E147` | a value in `akk.toml` is not valid (unknown mode, empty entry, ...) |
//! | `E148` | `[package] entry` does not name a file |
//! | `E149` | a `[dependencies]` entry cannot be resolved |

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::library::{self, LibraryLoader};

/// The manifest file's name, at the project root.
pub const FILE_NAME: &str = "akk.toml";

/// Build modes `[build] mode` accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildMode {
    Bundle,
    Bytecode,
}

/// `[package]` -- what the project is called and where it starts.
///
/// `version`/`description`/`authors`/`license` are read by `akk build`'s
/// metadata, which lands in the next phase; the manifest already keeps them
/// so a project file does not have to change then.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Package {
    /// `name` (default: the project folder's name).
    pub name: String,
    /// `version` (default: `0.1.0`).
    pub version: String,
    /// `entry`, relative to the project root (default: `main.akk`).
    pub entry: String,
    pub description: Option<String>,
    pub authors: Vec<String>,
    pub license: Option<String>,
}

/// `[build]` -- how `akk build` should package the project. Parsed and
/// validated now; `akk build` (the next phase) is what consumes it.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Build {
    pub mode: BuildMode,
    /// `output` (default: the package name).
    pub output: String,
    /// `out_dir`, relative to the project root (default: `build`).
    pub out_dir: PathBuf,
    pub optimize: bool,
    /// `[build.bundle] include` -- files/folders embedded in a bundle.
    pub include: Vec<String>,
}

/// `[run]` -- what a run of the project shows and writes.
#[derive(Debug, Clone)]
pub struct RunSettings {
    /// Write the `<file>.akop` transcript next to the program.
    pub transcript: bool,
    /// Show the static and runtime warnings.
    pub warnings: bool,
    /// Print the closing `Interpreted in ...` line.
    pub timing: bool,
}

/// Where a `[dependencies]` entry comes from. `Version`/`Git` are downloaded
/// by `akk install`; `Path` is resolved directly (see `resolve_dependencies`).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum DependencySource {
    /// `"*"` (or another version string): a package from the library repo,
    /// found where `akk install` puts it.
    Version(String),
    /// `{ path = "..." }`: a folder or `.akk` file inside (or beside) the
    /// project.
    Path(PathBuf),
    /// `{ git = "...", rev = "..." }`: `rev` is optional.
    Git { url: String, rev: Option<String> },
}

/// One `[dependencies]` entry.
#[derive(Debug, Clone)]
pub struct Dependency {
    pub name: String,
    pub source: DependencySource,
}

/// A dependency after resolution: the file it resolves to, or `None` for a
/// built-in library (compiled into `akk`, so there is no file).
#[derive(Debug, Clone)]
pub struct ResolvedDependency {
    pub name: String,
    pub path: Option<PathBuf>,
}

/// The whole parsed manifest, plus the root folder it was found in.
///
/// `path`, `build` and `windows_output` are what `akk build` reads next; the
/// run path only needs the root, entry, run settings and dependencies.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Manifest {
    /// Folder holding `akk.toml`; every relative path is resolved from here.
    pub root: PathBuf,
    /// The `akk.toml` file itself.
    pub path: PathBuf,
    pub package: Package,
    pub build: Build,
    pub run: RunSettings,
    pub dependencies: Vec<Dependency>,
    /// `[target.windows] output`, when given.
    pub windows_output: Option<String>,
    /// `W008` lines for keys this version of `akk` does not know.
    pub warnings: Vec<String>,
}

// --- serde shape of akk.toml ------------------------------------------------

#[derive(Deserialize, Default)]
struct RawManifest {
    package: Option<RawPackage>,
    build: Option<RawBuild>,
    run: Option<RawRun>,
    dependencies: Option<toml::Table>,
    target: Option<RawTarget>,
}

#[derive(Deserialize, Default)]
struct RawPackage {
    name: Option<String>,
    version: Option<String>,
    entry: Option<String>,
    description: Option<String>,
    authors: Option<Vec<String>>,
    license: Option<String>,
}

#[derive(Deserialize, Default)]
struct RawBuild {
    mode: Option<String>,
    output: Option<String>,
    out_dir: Option<String>,
    optimize: Option<bool>,
    bundle: Option<RawBundle>,
}

#[derive(Deserialize, Default)]
struct RawBundle {
    include: Option<Vec<String>>,
}

#[derive(Deserialize, Default)]
struct RawRun {
    transcript: Option<bool>,
    warnings: Option<bool>,
    timing: Option<bool>,
}

#[derive(Deserialize, Default)]
struct RawTarget {
    windows: Option<RawWindows>,
}

#[derive(Deserialize, Default)]
struct RawWindows {
    output: Option<String>,
}

#[derive(Deserialize)]
struct RawDependencyTable {
    path: Option<String>,
    git: Option<String>,
    rev: Option<String>,
}

/// The keys each table accepts. Anything else is a `W008` warning.
const TOP_LEVEL_KEYS: &[&str] = &["package", "build", "run", "dependencies", "target"];
const PACKAGE_KEYS: &[&str] = &["name", "version", "entry", "description", "authors", "license"];
const BUILD_KEYS: &[&str] = &["mode", "output", "out_dir", "optimize", "bundle"];
const BUNDLE_KEYS: &[&str] = &["include"];
const RUN_KEYS: &[&str] = &["transcript", "warnings", "timing"];
const TARGET_KEYS: &[&str] = &["windows"];
const WINDOWS_KEYS: &[&str] = &["output"];
const DEPENDENCY_KEYS: &[&str] = &["path", "git", "rev"];

/// Find the nearest `akk.toml`: the start folder first, then each parent.
/// `None` when there is no manifest above `start`.
pub fn discover(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start);
    while let Some(current) = dir {
        let candidate = current.join(FILE_NAME);
        if candidate.is_file() {
            return Some(candidate);
        }
        dir = current.parent();
    }
    None
}

/// Load and validate the manifest at `path`.
pub fn load(path: &Path) -> Result<Manifest, String> {
    let text = fs::read_to_string(path)
        .map_err(|e| format!("E146 akk.toml ကို ဖတ်၍မရပါ - {}", e))?;

    // Parse once as a `toml::Value` so the key walk below can see every key
    // that was written, then once through serde for the values themselves.
    let raw_value: toml::Value = toml::from_str(&text)
        .map_err(|e| format!("E146 akk.toml ကို parse လုပ်၍မရပါ - {}", e))?;
    let raw: RawManifest = toml::from_str(&text)
        .map_err(|e| format!("E147 akk.toml ၏ တန်ဖိုး မမှန်ပါ - {}", e))?;

    let root = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

    let warnings = unknown_keys(&raw_value);

    let package = parse_package(raw.package, &root)?;
    let build = parse_build(raw.build, &package)?;
    let run = raw.run.map(parse_run).unwrap_or_default();
    let windows_output = raw
        .target
        .and_then(|t| t.windows)
        .and_then(|w| w.output)
        .filter(|s| !s.trim().is_empty());
    let dependencies = parse_dependencies(raw.dependencies)?;

    Ok(Manifest {
        root,
        path: path.to_path_buf(),
        package,
        build,
        run,
        dependencies,
        windows_output,
        warnings,
    })
}

fn parse_package(raw: Option<RawPackage>, root: &Path) -> Result<Package, String> {
    let raw = raw.unwrap_or_default();
    let name = match raw.name {
        Some(name) if !name.trim().is_empty() => name,
        Some(_) => return Err(invalid_value("[package]", "name", "name သည် အလွတ် မဖြစ်ရပါ")),
        None => root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "akk-project".to_string()),
    };

    let entry = raw.entry.unwrap_or_else(|| "main.akk".to_string());
    if entry.trim().is_empty() {
        return Err(invalid_value(
            "[package]",
            "entry",
            "entry သည် အလွတ် မဖြစ်ရပါ",
        ));
    }

    Ok(Package {
        name,
        version: raw.version.unwrap_or_else(|| "0.1.0".to_string()),
        entry,
        description: raw.description.filter(|s| !s.trim().is_empty()),
        authors: raw.authors.unwrap_or_default(),
        license: raw.license.filter(|s| !s.trim().is_empty()),
    })
}

fn parse_build(raw: Option<RawBuild>, package: &Package) -> Result<Build, String> {
    let raw = raw.unwrap_or_default();

    let mode = match raw.mode.as_deref().unwrap_or("bundle") {
        "bundle" => BuildMode::Bundle,
        "bytecode" => BuildMode::Bytecode,
        other => {
            return Err(format!(
                "E147 akk.toml ၏ [build] mode \"{}\" ကို မသိပါ။ \"bundle\" သို့မဟုတ် \"bytecode\" ကို သုံးပါ။",
                other
            ))
        }
    };

    let out_dir = match raw.out_dir {
        Some(dir) if !dir.trim().is_empty() => PathBuf::from(dir),
        Some(_) => {
            return Err(invalid_value(
                "[build]",
                "out_dir",
                "out_dir သည် အလွတ် မဖြစ်ရပါ",
            ))
        }
        None => PathBuf::from("build"),
    };

    Ok(Build {
        mode,
        output: raw.output.unwrap_or_else(|| package.name.clone()),
        out_dir,
        optimize: raw.optimize.unwrap_or(true),
        include: raw.bundle.and_then(|b| b.include).unwrap_or_default(),
    })
}

fn parse_run(raw: RawRun) -> RunSettings {
    // Every setting is on unless the manifest turns it off.
    RunSettings {
        transcript: raw.transcript.unwrap_or(true),
        warnings: raw.warnings.unwrap_or(true),
        timing: raw.timing.unwrap_or(true),
    }
}

fn parse_dependencies(raw: Option<toml::Table>) -> Result<Vec<Dependency>, String> {
    let mut dependencies = Vec::new();
    let Some(table) = raw else {
        return Ok(dependencies);
    };

    for (name, value) in table {
        let source = if let Some(version) = value.as_str() {
            if version.trim().is_empty() {
                return Err(invalid_value(
                    "[dependencies]",
                    &name,
                    "ဗားရှင်း သို့မဟုတ် \"*\" ကို ပေးရပါမည်",
                ));
            }
            DependencySource::Version(version.to_string())
        } else {
            let detail = value
                .clone()
                .try_into::<RawDependencyTable>()
                .map_err(|_| {
                    invalid_value(
                        "[dependencies]",
                        &name,
                        "\"*\" သို့မဟုတ် { path = \"...\" } / { git = \"...\", rev = \"...\" } ကို ပေးပါ",
                    )
                })?;
            match (detail.path, detail.git) {
                (Some(path), None) if !path.trim().is_empty() => {
                    DependencySource::Path(PathBuf::from(path))
                }
                (None, Some(url)) if !url.trim().is_empty() => DependencySource::Git {
                    url,
                    rev: detail.rev.filter(|r| !r.trim().is_empty()),
                },
                (Some(_), Some(_)) => {
                    return Err(invalid_value(
                        "[dependencies]",
                        &name,
                        "path နှင့် git ကို တစ်ခုတည်း ရွေးပါ",
                    ))
                }
                _ => {
                    return Err(invalid_value(
                        "[dependencies]",
                        &name,
                        "path သို့မဟုတ် git ကို ပေးရပါမည်",
                    ))
                }
            }
        };
        dependencies.push(Dependency { name, source });
    }

    Ok(dependencies)
}

/// Every key that the manifest's schema does not know, as `W008` lines.
/// Unknown keys are never fatal -- a newer manifest must stay readable by an
/// older `akk` -- so they are collected for the warning printer instead.
fn unknown_keys(value: &toml::Value) -> Vec<String> {
    let mut warnings = Vec::new();
    let Some(top) = value.as_table() else {
        return warnings;
    };

    warn_unknown(top, TOP_LEVEL_KEYS, None, &mut warnings);
    if let Some(package) = top.get("package").and_then(|v| v.as_table()) {
        warn_unknown(package, PACKAGE_KEYS, Some("[package]"), &mut warnings);
    }
    if let Some(build) = top.get("build").and_then(|v| v.as_table()) {
        warn_unknown(build, BUILD_KEYS, Some("[build]"), &mut warnings);
        if let Some(bundle) = build.get("bundle").and_then(|v| v.as_table()) {
            warn_unknown(bundle, BUNDLE_KEYS, Some("[build.bundle]"), &mut warnings);
        }
    }
    if let Some(run) = top.get("run").and_then(|v| v.as_table()) {
        warn_unknown(run, RUN_KEYS, Some("[run]"), &mut warnings);
    }
    if let Some(target) = top.get("target").and_then(|v| v.as_table()) {
        warn_unknown(target, TARGET_KEYS, Some("[target]"), &mut warnings);
        if let Some(windows) = target.get("windows").and_then(|v| v.as_table()) {
            warn_unknown(windows, WINDOWS_KEYS, Some("[target.windows]"), &mut warnings);
        }
    }
    if let Some(dependencies) = top.get("dependencies").and_then(|v| v.as_table()) {
        for (name, entry) in dependencies {
            if let Some(detail) = entry.as_table() {
                warn_unknown(
                    detail,
                    DEPENDENCY_KEYS,
                    Some(&format!("[dependencies.{}]", name)),
                    &mut warnings,
                );
            }
        }
    }

    warnings
}

fn warn_unknown(
    table: &toml::Table,
    known: &[&str],
    section: Option<&str>,
    out: &mut Vec<String>,
) {
    for key in table.keys() {
        if known.contains(&key.as_str()) {
            continue;
        }
        out.push(match section {
            Some(section) => format!(
                "W008 akk.toml ၏ {} တွင် \"{}\" ဆိုသော key ကို မသိပါ (unknown key)။",
                section, key
            ),
            None => format!(
                "W008 akk.toml တွင် \"{}\" ဆိုသော key ကို မသိပါ (unknown key)။",
                key
            ),
        });
    }
}

fn invalid_value(section: &str, key: &str, detail: &str) -> String {
    format!(
        "E147 akk.toml ၏ {} \"{}\" မမှန်ပါ - {}။",
        section, key, detail
    )
}

impl Manifest {
    /// The entry program, resolved from the project root.
    pub fn entry_path(&self) -> PathBuf {
        self.root.join(&self.package.entry)
    }

    /// The entry file must exist before a run starts (`E148`).
    pub fn validate_entry(&self) -> Result<(), String> {
        if self.entry_path().is_file() {
            Ok(())
        } else {
            Err(format!(
                "E148 akk.toml ၏ [package] entry \"{}\" ဖိုင်ကို \"{}\" တွင် ရှာမတွေ့ပါ။",
                self.package.entry,
                self.root.display()
            ))
        }
    }

    /// Resolve every `[dependencies]` entry, in the order the manifest
    /// documents: a `path` entry first, then a sibling `<name>.akk`, then a
    /// downloaded package under `libraries/<name>/`, then a built-in.
    ///
    /// The returned list is what the interpreter registers, so `path`
    /// dependencies also take priority when the program imports them.
    pub fn resolve_dependencies(
        &self,
        libraries_dir: &Path,
    ) -> Result<Vec<ResolvedDependency>, String> {
        let mut loader = LibraryLoader::new(libraries_dir.to_path_buf());
        loader.add_script_dir(self.root.clone());

        // A `path` dependency is checked directly first: "the folder is not
        // there" is a different problem from "the package is not installed",
        // and its message can name the path that was written.
        for dependency in &self.dependencies {
            if let DependencySource::Path(path) = &dependency.source {
                let full = self.root.join(path);
                if !full.exists() {
                    return Err(unresolved(
                        &dependency.name,
                        &format!("\"{}\" path ကို ရှာမတွေ့ပါ", full.display()),
                    ));
                }
                loader.add_dependency(&dependency.name, full);
            }
        }

        let mut resolved = Vec::new();
        for dependency in &self.dependencies {
            match loader.resolve_dependency(&dependency.name) {
                Some(library::DependencyResolution::Builtin) => {
                    resolved.push(ResolvedDependency {
                        name: dependency.name.clone(),
                        path: None,
                    });
                }
                Some(library::DependencyResolution::Source(path)) => {
                    resolved.push(ResolvedDependency {
                        name: dependency.name.clone(),
                        path: Some(path),
                    });
                }
                None => return Err(unresolved(&dependency.name, "")),
            }
        }
        Ok(resolved)
    }
}

fn unresolved(name: &str, detail: &str) -> String {
    let hint = if detail.is_empty() {
        String::new()
    } else {
        format!(" ({})", detail)
    };
    format!(
        "E149 akk.toml ၏ dependency \"{}\" ကို ရှာမတွေ့ပါ{}။ \"akk install {}\" ဖြင့် ထည့်သွင်းပါ၊ [dependencies] တွင် path ကို သတ်မှတ်ပါ၊ သို့မဟုတ် \"{}.akk\" ဖိုင်ကို project root တွင် ထားပါ။",
        name, hint, name, name
    )
}

impl Default for RunSettings {
    /// The settings a run uses when there is no `[run]` table: everything on,
    /// exactly like `akk file.akk` has always behaved.
    fn default() -> Self {
        RunSettings {
            transcript: true,
            warnings: true,
            timing: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_manifest(tag: &str, text: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "akk-manifest-{}-{}",
            tag,
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp folder");
        fs::write(dir.join(FILE_NAME), text).expect("manifest");
        dir.join(FILE_NAME)
    }

    #[test]
    fn defaults_fill_every_optional_field() {
        let path = write_manifest("defaults", "");
        let manifest = load(&path).expect("empty manifest");
        assert_eq!(manifest.package.entry, "main.akk");
        assert_eq!(manifest.build.mode, BuildMode::Bundle);
        assert_eq!(manifest.build.out_dir, PathBuf::from("build"));
        assert!(manifest.build.optimize);
        assert!(manifest.run.transcript);
        assert!(manifest.run.warnings);
        assert!(manifest.run.timing);
        assert!(manifest.dependencies.is_empty());
        fs::remove_dir_all(manifest.root).ok();
    }

    #[test]
    fn unknown_keys_are_warnings_not_errors() {
        let path = write_manifest(
            "unknown",
            "[package]\nname = \"p\"\ncolour = \"red\"\n\n[build]\nmode = \"bytecode\"\n[build.bundle]\ninclude = [\"assets\"]\n",
        );
        let manifest = load(&path).expect("manifest with an unknown key");
        assert_eq!(manifest.build.mode, BuildMode::Bytecode);
        assert!(
            manifest
                .warnings
                .iter()
                .any(|w| w.contains("W008") && w.contains("colour")),
            "{:?}",
            manifest.warnings
        );
        fs::remove_dir_all(manifest.root).ok();
    }

    #[test]
    fn bad_mode_is_e147() {
        let path = write_manifest("mode", "[build]\nmode = \"fast\"\n");
        let err = load(&path).expect_err("bad mode");
        assert!(err.starts_with("E147"), "{}", err);
        fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn discover_walks_to_parent_folders() {
        let path = write_manifest("discover", "");
        let nested = path.parent().unwrap().join("a").join("b");
        fs::create_dir_all(&nested).expect("nested");
        assert_eq!(discover(&nested), Some(path.clone()));
        fs::remove_dir_all(path.parent().unwrap()).ok();
    }
}
