//! `akk build`: write one executable that runs the project.

use super::run::write_warnings;
use super::*;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// The path `akk build` writes for a project: `[build] out_dir` plus
/// `[build] output` (or `[target.windows] output` on Windows) and the
/// platform's executable suffix.
pub(crate) fn manifest_output_path(manifest: &manifest::Manifest) -> Result<PathBuf, String> {
    let mut name = manifest.build.output.clone();
    if cfg!(windows) {
        if let Some(windows) = &manifest.windows_output {
            if !windows.is_empty() {
                name = windows.clone();
            }
        }
    }
    check_output_name(&name)?;
    let suffix = std::env::consts::EXE_SUFFIX;
    if !suffix.is_empty() && !name.ends_with(suffix) {
        name.push_str(suffix);
    }
    Ok(manifest.root.join(&manifest.build.out_dir).join(name))
}

/// The scripts a project's `[dependencies]` point at, for cache freshness.
pub(crate) fn dependency_paths(manifest: &manifest::Manifest) -> Vec<PathBuf> {
    manifest
        .resolve_dependencies(&libraries_dir())
        .map(|resolved| {
            dependency_pairs(resolved)
                .into_iter()
                .map(|(_, path)| path)
                .collect()
        })
        .unwrap_or_default()
}

/// Newest modification time among everything a build reads: every `.akk` file
/// and `akk.toml` under `root` (skipping the build output folder), `extra`
/// files, and the running `akk` itself -- so a new compiler invalidates output
/// it did not write.
pub(crate) fn newest_input(
    root: &Path,
    skip: Option<&Path>,
    extra: &[PathBuf],
) -> Option<std::time::SystemTime> {
    fn walk(dir: &Path, skip: Option<&Path>, newest: &mut Option<std::time::SystemTime>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if skip.map(|s| path == s).unwrap_or(false) {
                continue;
            }
            if path.is_dir() {
                walk(&path, skip, newest);
                continue;
            }
            let wanted = path.extension().map(|e| e == "akk").unwrap_or(false)
                || path.file_name().map(|n| n == "akk.toml").unwrap_or(false);
            if !wanted {
                continue;
            }
            if let Ok(time) = fs::metadata(&path).and_then(|m| m.modified()) {
                if newest.map(|n| time > n).unwrap_or(true) {
                    *newest = Some(time);
                }
            }
        }
    }

    let mut newest = None;
    walk(root, skip, &mut newest);
    let mut consider = |time: std::time::SystemTime| {
        if newest.map(|n| time > n).unwrap_or(true) {
            newest = Some(time);
        }
    };
    for path in extra {
        if let Ok(time) = fs::metadata(path).and_then(|m| m.modified()) {
            consider(time);
        }
    }
    if let Ok(exe) = env::current_exe() {
        if let Ok(time) = fs::metadata(exe).and_then(|m| m.modified()) {
            consider(time);
        }
    }
    newest
}

/// Whether `out` already reflects every input a build reads.
pub(crate) fn build_is_current(
    out: &Path,
    root: &Path,
    skip: Option<&Path>,
    extra: &[PathBuf],
) -> bool {
    let Ok(out_time) = fs::metadata(out).and_then(|m| m.modified()) else {
        return false;
    };
    match newest_input(root, skip, extra) {
        Some(newest) => out_time >= newest,
        None => true,
    }
}

/// The `build` folder `akk run <file>` keeps its compiled file in: `build/`
/// beside the source, created on demand.
pub(crate) fn single_file_build_dir(file: &str) -> PathBuf {
    Path::new(file)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
        .join("build")
}

/// The file `akk run <file>` runs: `<file_name>` plus the platform's
/// executable suffix (`.exe` on Windows), inside that `build` folder.
pub(crate) fn single_file_cache_path(file: &str) -> PathBuf {
    let path = Path::new(file);
    let mut name = path
        .file_stem()
        .map(|stem| stem.to_os_string())
        .unwrap_or_else(|| std::ffi::OsString::from("program"));
    name.push(std::env::consts::EXE_SUFFIX);
    single_file_build_dir(file).join(name)
}

/// The executable `akk build <file>` writes: `<file_name>` plus the
/// platform's executable suffix (`.exe` on Windows), beside the source.
pub(crate) fn single_file_output(file: &str) -> PathBuf {
    let path = Path::new(file);
    let mut name = path
        .file_stem()
        .map(|stem| stem.to_os_string())
        .unwrap_or_else(|| std::ffi::OsString::from("program"));
    name.push(std::env::consts::EXE_SUFFIX);
    path.with_file_name(name)
}

/// A sibling of `file` whose stem carries `suffix`: `hello.akk` with
/// `"_info.akop"` gives `hello_info.akop`.
fn with_stem_suffix(file: &Path, suffix: &str) -> PathBuf {
    let mut name = file
        .file_stem()
        .map(|stem| stem.to_os_string())
        .unwrap_or_else(|| std::ffi::OsString::from("program"));
    name.push(suffix);
    file.with_file_name(name)
}

/// The diagnostics file a failed `akk build <file>` leaves beside the source:
/// `<file_name>_info.akop`. A build that succeeds writes none.
pub(crate) fn single_file_info(file: &str) -> PathBuf {
    with_stem_suffix(Path::new(file), "_info.akop")
}

/// The diagnostics file a failed project build leaves: `<output>_info.akop`
/// beside the executable in its `out_dir`, so both forms of `akk build` use
/// the same stem convention for their report.
pub(crate) fn project_info(out: &Path) -> PathBuf {
    with_stem_suffix(out, "_info.akop")
}

/// The static warnings for `source`: the same pass a run starts with. `akk
/// build` shows no warnings itself, so they are collected only for the
/// diagnostics file a failed build leaves. A file that does not lex or parse
/// has no warnings -- its error is the whole report.
fn compile_warnings(source: &str, file: &str) -> Vec<String> {
    let Ok(tokens) = lexer::lex(source) else {
        return Vec::new();
    };
    let Ok(stmts) = parser::parse(&tokens) else {
        return Vec::new();
    };
    let mut search_dirs = Vec::new();
    if let Some(parent) = Path::new(file)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
    {
        search_dirs.push(parent.to_path_buf());
    }
    if let Ok(cwd) = env::current_dir() {
        if !search_dirs.iter().any(|dir| *dir == cwd) {
            search_dirs.push(cwd);
        }
    }
    crate::sema::check::library_warnings(libraries_dir(), &stmts, &search_dirs, &[])
}

/// The text an info file holds: the static warnings first (a run shows them
/// before the program starts), then the error that stopped the build. Both
/// build paths write this same format.
fn info_text(warnings: &[String], error: &str) -> String {
    let mut log = String::new();
    for warning in warnings {
        log.push_str(warning);
        log.push('\n');
    }
    log.push_str(error);
    log.push('\n');
    log
}

/// Write the diagnostics file at `path`. Failing to write it must not hide the
/// build's own error, so the failure is reported and the build still exits
/// nonzero.
fn write_info(path: &Path, warnings: &[String], error: &str) {
    // The project's `out_dir` may not exist yet when the first build fails;
    // the info file still has to be written beside where the executable would
    // have gone.
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if let Err(e) = fs::write(path, info_text(warnings, error)) {
        eprintln!("ဖိုင် \"{}\" ကို ရေး၍မရပါ - {}", plain_path(path), e);
    }
}

/// `akk build <file>` -- compile one script to a native executable for this
/// machine, beside the source.
///
/// This is the machine-code path: the slice the backend supports (int, float,
/// bool and string values, arithmetic, comparisons, print, functions, calls,
/// `return`, and raising with `try/catch`) becomes a real executable written
/// without an assembler or linker. Anything outside it is refused with `E177`
/// before a byte is written, so no executable appears for a program the
/// backend cannot compile; instead a failed build writes `<file_name>_info.akop`
/// beside the source with the warnings it found and the error itself.
pub(crate) fn cmd_build_file(file: &str, optimize: bool) -> i32 {
    let src = match fs::read_to_string(file) {
        Ok(src) => src,
        Err(e) => {
            eprintln!("ဖိုင် \"{}\" ကို ဖတ်၍မရပါ - {}", file, e);
            return 1;
        }
    };
    let out = single_file_output(file);
    match crate::backend::build_source(&src, &out, optimize) {
        Ok(()) => {
            // A program that now compiles leaves no stale diagnostics file
            // from an earlier failed build.
            let _ = fs::remove_file(single_file_info(file));
            println!("{}", plain_path(&out));
            0
        }
        Err(e) => {
            eprintln!("{}", e);
            let warnings = compile_warnings(&src, file);
            write_info(&single_file_info(file), &warnings, &e);
            1
        }
    }
}

/// `akk build` -- write one executable that runs this project.
///
/// The output is a copy of this interpreter with the program's source, and the
/// scripts it brings in, appended after it. It is not machine code and it does
/// not run faster; a built file only saves typing `akk run`. Building never
/// runs the program and writes no `.akop`.
pub(crate) fn cmd_build(no_warn: bool) -> i32 {
    let cwd = match env::current_dir() {
        Ok(cwd) => cwd,
        Err(e) => {
            eprintln!("E168 လက်ရှိ ဖိုဒါကို မသိပါ - {}", e);
            return 1;
        }
    };

    let manifest = match manifest::discover(&cwd) {
        Some(path) => match manifest::load(&path) {
            Ok(manifest) => manifest,
            Err(e) => {
                eprintln!("{}", e);
                return 1;
            }
        },
        None => {
            eprintln!("error: akk.toml ကို `{}` ၌ မတွေ့ပါ။", plain_path(&cwd));
            return 1;
        }
    };

    // A built file is a copy of this executable plus appended bytes. On macOS
    // that would invalidate the arm64 signature, and no other platform has
    // been tried, so only Windows and Linux are offered.
    if !cfg!(windows) && !cfg!(target_os = "linux") {
        eprintln!(
            "E165 ဤ ပလက်ဖောင်း ({}) တွင် ထုပ်ပိုးဖိုင်ကို မထောက်ပံ့ပါ။ Windows သို့မဟုတ် Linux တွင် build ပါ။",
            std::env::consts::OS
        );
        return 1;
    }

    // `mode = "elf"` never reaches here: `parse_build` refuses an unknown mode
    // with E147 while the manifest loads. `bundle` (and its `interpreter`
    // spelling) embeds the source; `bytecode` embeds compiled instructions.
    let bytecode_mode = matches!(manifest.build.mode, manifest::BuildMode::Bytecode);

    if !manifest.build.include.is_empty() {
        eprintln!(
            "E164 [build.bundle] include ကို ဤ ဗားရှင်းတွင် မထောက်ပံ့သေးပါ။ ပါဝင်စေလိုသော ဖိုင်များကို ကိုယ်တိုင် ကူးထည့်ပါ။"
        );
        return 1;
    }

    let out = match manifest_output_path(&manifest) {
        Ok(out) => out,
        Err(e) => {
            eprintln!("{}", e);
            return 1;
        }
    };

    // `akk build` does nothing when the artifact already matches every input.
    // That is a successful build too, so a diagnostics file an earlier failed
    // build left must not survive it.
    let dependencies_now = dependency_paths(&manifest);
    let build_dir = manifest.root.join(&manifest.build.out_dir);
    if build_is_current(&out, &manifest.root, Some(&build_dir), &dependencies_now) {
        let _ = fs::remove_file(project_info(&out));
        println!("Already compiled {}", plain_path(&out));
        return 0;
    }
    let started = std::time::Instant::now();

    if let Err(e) = manifest.validate_entry() {
        eprintln!("{}", e);
        return 1;
    }
    let dependencies = match manifest.resolve_dependencies(&libraries_dir()) {
        Ok(resolved) => dependency_pairs(resolved),
        Err(e) => {
            eprintln!("{}", e);
            return 1;
        }
    };

    // The header exists once every option has validated, the same way
    // `akk run` builds it after resolution succeeds: a rejected manifest
    // prints no `Compiling` line.
    let project = Some(progress::ProjectHeader {
        name: manifest.package.name.clone(),
        version: manifest.package.version.clone(),
        root: plain_path(
            &fs::canonicalize(&manifest.root).unwrap_or_else(|_| manifest.root.clone()),
        ),
    });
    let reporter = progress::Reporter::new(project);
    // `Compiling`: the entry is read, lexed and parsed.
    reporter.compiling();

    // From here on the program itself is compiled, and every failure leaves
    // `<output>_info.akop` beside the executable: the warnings the static
    // pass produced (none until it has run) then the error that stopped the
    // build. The same text the single-file path writes.
    let info = project_info(&out);
    let fail = |warnings: &[String], error: &str| -> i32 {
        eprintln!("{}", error);
        write_info(&info, warnings, error);
        1
    };

    let filename = manifest.entry_path().display().to_string();
    let src = match fs::read_to_string(&filename) {
        Ok(src) => src,
        Err(e) => {
            let message = format!("ဖိုင် \"{}\" ကို ဖတ်၍မရပါ - {}", filename, e);
            return fail(&[], &message);
        }
    };
    let tokens = match lexer::lex_source(&src) {
        Ok(tokens) => tokens,
        Err(e) => return fail(&[], &e),
    };
    let stmts = match parser::parse_source(&tokens, &src) {
        Ok(stmts) => stmts,
        Err(e) => return fail(&[], &e),
    };

    // The static phase: the same folders a run searches, plus the embed scan.
    // A run looks beside the entry file first (usually `src/`), so the embed
    // scan has to look there too or a neighbouring script is never carried.
    let script_dir = manifest
        .entry_path()
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| manifest.root.clone());
    let mut search_dirs = vec![script_dir.clone()];
    if script_dir != manifest.root {
        search_dirs.push(manifest.root.clone());
    }
    if let Ok(cwd) = env::current_dir() {
        if !search_dirs.iter().any(|dir| *dir == cwd) {
            search_dirs.push(cwd);
        }
    }

    let mut embedded = match collect_embedded(&stmts, &manifest.root, &search_dirs, &dependencies) {
        Ok(scripts) => scripts,
        Err(e) => return fail(&[], &e),
    };
    // Dependencies first: a name that is both a path dependency and a
    // neighbouring script resolves to the dependency at run time.
    embedded.sort_by_key(|script| script.kind);

    let static_warnings = if no_warn || !manifest.run.warnings {
        Vec::new()
    } else {
        let mut all = manifest.warnings.clone();
        all.extend(crate::sema::check::library_warnings(
            libraries_dir(),
            &stmts,
            &search_dirs,
            &dependencies,
        ));
        all
    };
    reporter.building_done();
    let _ = write_warnings(&static_warnings);

    // bit0 has deps, bit1 transcript, bit2 warnings, bit3 timing. `--no-warn`
    // at build time bakes the warnings off; it can still be given at run time.
    let has_deps: u32 = if embedded.is_empty() { 0 } else { 0b0001 };
    let transcript_bit: u32 = if manifest.run.transcript { 0b0010 } else { 0 };
    let warnings_bit: u32 = if manifest.run.warnings && !no_warn {
        0b0100
    } else {
        0
    };
    let timing_bit: u32 = if manifest.run.timing { 0b1000 } else { 0 };
    let run_flags = has_deps | transcript_bit | warnings_bit | timing_bit;

    // `[build] mode = "bytecode"`: compile here, so a program the VM cannot
    // run fails the build with the compiler's own E170 instead of producing a
    // file that behaves differently from `akk <file>`.
    let compiled = if bytecode_mode {
        if !embedded.is_empty() {
            return fail(
                &static_warnings,
                "E170 [build] mode = \"bytecode\" သည် နည်းပညာ (library) များကို မထောက်ပံ့သေးပါ။ \"bundle\" ကို သုံးပါ။",
            );
        }
        match bytecode::compile_with_source(&stmts, &src).and_then(|program| program.encode()) {
            Ok(bytes) => Some(bytes),
            Err(e) => return fail(&static_warnings, &e),
        }
    } else {
        None
    };

    let input = bundle::BuildInput {
        source: &src,
        bytecode: compiled.as_deref(),
        deps: &embedded,
        run_flags,
    };
    let bytes = match bundle::write_bundle(&out, &input) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("{}", e);
            return 1;
        }
    };

    // A clean build leaves no diagnostics behind, even from an earlier
    // failed one.
    let _ = fs::remove_file(&info);

    // `Compiled in` first, then the file it wrote: the order the header lists.
    progress::compiled_in(started.elapsed().as_secs_f64());
    reporter.finished(&plain_path(&out), bytes);
    0
}

/// `[build] output` / `[target.windows] output` must be one file name: no
/// folder, no `..`, not empty. `out_dir` is the only way to choose a folder.
pub(crate) fn check_output_name(name: &str) -> Result<(), String> {
    let mut parts = Path::new(name).components();
    let one_name =
        matches!(parts.next(), Some(std::path::Component::Normal(_))) && parts.next().is_none();
    if one_name {
        Ok(())
    } else {
        Err(format!(
            "E147 akk.toml ၏ [build] output \"{}\" သည် ဖိုင်အမည်တစ်ခုတည်း မဟုတ်ပါ။ ဖိုဒါအတွက် [build] out_dir ကို သုံးပါ။",
            name
        ))
    }
}

/// Walk the `use` statements from the entry and collect every script that can
/// be found now: `[dependencies] path` sources first (kind 0), then
/// neighbouring `<name>.akk` scripts (kind 1). A name that is not found is left
/// to run time, which reports it as usual; a script that is found but cannot be
/// read is E163, because a run would have used it. Downloaded packages under
/// `libraries/` are never embedded.
pub(crate) fn collect_embedded(
    stmts: &[parser::Stmt],
    project_root: &Path,
    search_dirs: &[PathBuf],
    dependencies: &[(String, PathBuf)],
) -> Result<Vec<bundle::EmbeddedScript>, String> {
    let mut loader = library::LibraryLoader::new(libraries_dir());
    loader.add_script_dir(project_root.to_path_buf());
    for dir in search_dirs {
        if dir != project_root {
            loader.add_script_dir(dir.clone());
        }
    }
    for (name, path) in dependencies {
        loader.add_dependency(name, path.clone());
    }

    let mut found: Vec<bundle::EmbeddedScript> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut queue: Vec<String> = Vec::new();
    push_uses(stmts, &mut queue);

    while let Some(name) = queue.pop() {
        let key = name.strip_suffix(".akk").unwrap_or(&name).to_string();
        if !seen.insert(key.clone()) {
            continue;
        }
        let Some(path) = loader.embeddable_path(&name) else {
            continue;
        };
        let source = match fs::read_to_string(&path) {
            Ok(source) => source,
            Err(e) => {
                return Err(format!("E163 \"{}\" ကို ဖတ်၍မရပါ - {}", plain_path(&path), e));
            }
        };
        // The script's own `use` lines are embedded too, transitively. A
        // cycle stops here (the visited set); run time still reports E119.
        if let Ok(tokens) = lexer::lex(&source) {
            if let Ok(inner) = parser::parse(&tokens) {
                push_uses(&inner, &mut queue);
            }
        }
        let kind = if dependencies.iter().any(|(dep, _)| dep == &key) {
            bundle::KIND_DEPENDENCY
        } else {
            bundle::KIND_SCRIPT
        };
        found.push(bundle::EmbeddedScript {
            kind,
            name: key,
            source,
        });
    }

    Ok(found)
}

pub(crate) fn push_uses(stmts: &[parser::Stmt], queue: &mut Vec<String>) {
    for stmt in stmts {
        if let parser::Stmt::UseLibrary { libs, .. } = stmt {
            for (name, _alias) in libs {
                queue.push(name.clone());
            }
        }
    }
}

/// The `(name, path)` pairs the interpreter registers for `[dependencies]`
/// entries that resolved to a source file. Built-ins have no file to load.
pub(crate) fn dependency_pairs(
    resolved: Vec<manifest::ResolvedDependency>,
) -> Vec<(String, PathBuf)> {
    resolved
        .into_iter()
        .filter_map(|dependency| dependency.path.map(|path| (dependency.name, path)))
        .collect()
}
