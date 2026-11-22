//! Running a program: read, lex, parse, check, interpret.

use super::build::*;
use super::*;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

/// Saves everything the run showed on the terminal next to the program as
/// `<file name>.akop`: the output collected while the program ran, followed
/// by the error message when it fails (nothing extra when it succeeds). It is
/// written on every run, so a failing program leaves its `.akop` behind as
/// well.
pub(crate) fn save_terminal_log(filename: &str, output: Option<String>, closing_line: &str) {
    // Appended as-is, without forcing a line break: a program that ends in a
    // prompt (printed without a trailing newline) shows its closing line on
    // the same terminal line, and the log keeps that text.
    let mut log = output.unwrap_or_default();
    if closing_line.is_empty() {
        // No closing line: the transcript is the program's own output, ending
        // with the newline it already has.
        if !log.is_empty() && !log.ends_with('\n') {
            log.push('\n');
        }
    } else {
        log.push_str(closing_line);
        log.push('\n');
    }
    let out_path = PathBuf::from(filename).with_extension("akop");
    let first = match fs::write(&out_path, &log) {
        Ok(()) => return,
        Err(e) => e,
    };
    // A built file writes beside itself first. When that folder is not
    // writable, the same `<name>.akop` in the current directory is tried
    // before the run gives up on the transcript.
    if let Some(fallback) = out_path.file_name().map(PathBuf::from) {
        match fs::write(&fallback, &log) {
            Ok(()) => return,
            Err(e) => {
                eprintln!("ဖိုင် \"{}\" ကို ရေး၍မရပါ - {}", out_path.display(), first);
                eprintln!("ဖိုင် \"{}\" ကို ရေး၍မရပါ - {}", fallback.display(), e);
                return;
            }
        }
    }
    eprintln!("ဖိုင် \"{}\" ကို ရေး၍မရပါ - {}", out_path.display(), first);
}

/// What a run shows and writes. `akk file.akk` gets every setting on; `akk
/// run` takes them from the manifest's `[run]` table.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RunOptions {
    /// Write the `<file>.akop` transcript (including after a failure).
    transcript: bool,
    /// Show the static and runtime warnings.
    warnings: bool,
}

impl RunOptions {
    /// The historical behavior of `akk file.akk` (`--no-warn` turns only the
    /// warnings off).
    pub(crate) fn defaults(no_warn: bool) -> Self {
        RunOptions {
            transcript: true,
            warnings: !no_warn,
        }
    }

    /// A manifest's `[run]` settings, with `--no-warn` still winning.
    pub(crate) fn from_manifest(run: &manifest::RunSettings, no_warn: bool) -> Self {
        RunOptions {
            transcript: run.transcript,
            warnings: run.warnings && !no_warn,
        }
    }

    /// The settings a built file baked in at `akk build` time (the header
    /// flags in `bundle`); `--no-warn` still wins on the command line.
    pub(crate) fn from_flags(flags: u32) -> Self {
        RunOptions {
            transcript: flags & 0b0010 != 0,
            warnings: flags & 0b0100 != 0,
        }
    }
}

/// Run one program file, honoring `options`, and return the exit code the
/// process should end with. This is the whole plain run path: `akk file.akk`
/// and `akk run` both come through here, so the two behave identically apart
/// from the settings, the extra manifest warnings and `project` -- the header
/// lines only a project run prints.
pub(crate) fn run_program(
    filename: &str,
    options: RunOptions,
    extra_warnings: &[String],
    dependencies: &[(String, PathBuf)],
    project: Option<progress::ProjectHeader>,
) -> i32 {
    let reporter = progress::Reporter::new(project);
    // `Compiling`: the entry file is about to be read, lexed and parsed.
    reporter.compiling();

    let src = match fs::read_to_string(filename) {
        Ok(s) => s,
        Err(e) => {
            let msg = format!("ဖိုင် \"{}\" ကို ဖတ်၍မရပါ - {}", filename, e);
            eprintln!("{}", msg);
            if options.transcript {
                save_terminal_log(filename, None, &msg);
            }
            return 1;
        }
    };

    // Let this program import a neighboring `<name>.akk` script with
    // `နည်းပညာများ <name> ကို သုံးမည်။`: search the program's own folder
    // first, then the current working directory, before the libraries/
    // folder next to the binary. The static warning pass uses the same
    // folders, so a script's own source is checked too.
    let script_dir = PathBuf::from(filename)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    let cwd = env::current_dir().ok();

    let mut search_dirs = vec![script_dir];
    if let Some(cwd) = cwd {
        search_dirs.push(cwd);
    }

    run_source(
        filename,
        src,
        search_dirs,
        options,
        extra_warnings,
        dependencies,
        &reporter,
    )
}

/// The shared run path. `run_program` reads the file first; a built executable
/// hands over the source it carries instead, so both go through exactly these
/// steps. `anchor` is the path the `.akop` transcript is written beside, and
/// the reporter carries the header lines (`None` for a built file, which
/// prints none of them).
pub(crate) fn run_source(
    anchor: &str,
    src: String,
    search_dirs: Vec<PathBuf>,
    options: RunOptions,
    extra_warnings: &[String],
    dependencies: &[(String, PathBuf)],
    reporter: &progress::Reporter,
) -> i32 {
    let libs_dir = libraries_dir();

    let tokens = match lexer::lex_source(&src) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{}", e);
            if options.transcript {
                save_terminal_log(anchor, None, &e);
            }
            return 1;
        }
    };

    let stmts = match parser::parse_source(&tokens, &src) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{}", e);
            if options.transcript {
                save_terminal_log(anchor, None, &e);
            }
            return 1;
        }
    };

    // `Building`: the static phase. `library_warnings` reads and parses every
    // script this program brings in, so it is part of this phase and runs
    // before the `Building` line is printed. The warnings it produces are
    // shown next (and saved into the `.akop` transcript), and they never
    // change what the program does.
    let static_warnings = if options.warnings {
        let mut all: Vec<String> = extra_warnings.to_vec();
        all.extend(crate::sema::check::library_warnings(
            libraries_dir(),
            &stmts,
            &search_dirs,
            dependencies,
        ));
        all
    } else {
        Vec::new()
    };
    reporter.building_done();
    // `write_warnings` prints each line to stderr and hands back the text the
    // transcript starts with; it is empty when there is nothing to show.
    let warning_prefix = write_warnings(&static_warnings);

    // `Running`: the parsed program is about to execute.
    reporter.running();
    let outcome = match interpret(
        stmts,
        src,
        libs_dir,
        search_dirs,
        dependencies.to_vec(),
        false,
        !options.warnings,
    ) {
        Ok(outcome) => outcome,
        Err((message, code)) => {
            eprintln!("{}", message);
            return code;
        }
    };

    if let Some(e) = outcome.error {
        eprintln!("{}", e);
        // A failing run still saves its transcript: what the program managed
        // to print, then the error the terminal showed.
        if options.transcript {
            save_terminal_log(anchor, with_warnings(&warning_prefix, outcome.output), &e);
        }
        return 1;
    }

    // No runtime timer is printed: the only timing line is the `Compiled in`
    // one a build (or a run that had to compile) shows. The transcript still
    // ends with its own newline (see `save_terminal_log`).
    let closing_line = String::new();

    // Save everything the terminal showed next to the program as
    // `<file_name>.akop`: the warnings, the program's output and that closing
    // line.
    if options.transcript {
        save_terminal_log(
            anchor,
            with_warnings(&warning_prefix, outcome.output),
            &closing_line,
        );
    }
    0
}

/// `akk run [file]` -- the manifest-aware run.
///
/// With no file, the nearest `akk.toml` supplies the entry and `[run]`
/// settings. With a file, that file runs instead, but still takes the
/// manifest's settings and dependencies when there is one. With neither a
/// file nor a manifest there is nothing to run.
pub(crate) fn cmd_run(no_warn: bool, file: Option<&str>) -> i32 {
    let cwd = match env::current_dir() {
        Ok(cwd) => cwd,
        Err(e) => {
            eprintln!("E146 akk.toml ကို ရှာရာတွင် အမှား ဖြစ်ပေါ်ခဲ့ပါသည် - {}", e);
            return 1;
        }
    };

    let found = match manifest::discover(&cwd) {
        Some(path) => match manifest::load(&path) {
            Ok(manifest) => Some(manifest),
            Err(e) => {
                eprintln!("{}", e);
                return 1;
            }
        },
        None => None,
    };

    // `akk run` runs the compiled executable when it is current and compiles
    // first when it is not: a project uses `build/<name>`, an explicit file
    // uses `<file_name>.exe` beside it.
    if let Some(file) = file {
        return run_compiled_file(file, no_warn, found.as_ref());
    }
    if let Some(manifest) = found.as_ref() {
        return run_compiled_project(manifest, no_warn);
    }

    let (filename, options, warnings, dependencies, project) = match (found, file) {
        (Some(manifest), None) => {
            if let Err(e) = manifest.validate_entry() {
                eprintln!("{}", e);
                return 1;
            }
            let dependencies = match manifest.resolve_dependencies(&libraries_dir()) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("{}", e);
                    return 1;
                }
            };
            // The header exists only on this arm: `akk run` in project mode
            // with no file argument. It is built after resolution succeeded,
            // so a dependency failure still prints with no `Compiling` line.
            let project = Some(progress::ProjectHeader {
                name: manifest.package.name.clone(),
                version: manifest.package.version.clone(),
                root: plain_path(
                    &fs::canonicalize(&manifest.root).unwrap_or_else(|_| manifest.root.clone()),
                ),
            });
            (
                manifest.entry_path().display().to_string(),
                RunOptions::from_manifest(&manifest.run, no_warn),
                manifest.warnings.clone(),
                dependency_pairs(dependencies),
                project,
            )
        }
        (Some(manifest), Some(file)) => {
            let dependencies = match manifest.resolve_dependencies(&libraries_dir()) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("{}", e);
                    return 1;
                }
            };
            (
                file.to_string(),
                RunOptions::from_manifest(&manifest.run, no_warn),
                manifest.warnings.clone(),
                dependency_pairs(dependencies),
                None,
            )
        }
        (None, Some(file)) => (
            file.to_string(),
            RunOptions::defaults(no_warn),
            Vec::new(),
            Vec::new(),
            None,
        ),
        (None, None) => {
            eprintln!("error: akk.toml ကို `{}` ၌ မတွေ့ပါ။", plain_path(&cwd));
            return 1;
        }
    };

    run_program(&filename, options, &warnings, &dependencies, project)
}

/// Run the program appended to this executable. `argv` is ignored except for
/// the literal `--no-warn`: `myapp update` and `myapp build` run the program,
/// not the updater or the builder. No header lines are printed, no `libraries/`
/// folder is created, and the `.akop` transcript is written beside the
/// executable.
pub(crate) fn run_bundle(program: bundle::Bundle) -> i32 {
    let no_warn = env::args().skip(1).any(|a| a == "--no-warn");
    let mut options = RunOptions::from_flags(program.flags);
    if no_warn {
        options.warnings = false;
    }

    let anchor = plain_path(&program.anchor);
    let exe_dir = program
        .anchor
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

    // Frozen order for a built file: the embedded scripts (the loader checks
    // them before any folder) first, then a script in the current directory,
    // then `<exe dir>/libraries`, then a built-in library.
    let mut search_dirs = Vec::new();
    if let Ok(cwd) = env::current_dir() {
        search_dirs.push(cwd);
    }
    search_dirs.push(exe_dir);

    // A bytecode bundle carries instructions, not source: decode them and run
    // the VM. Nothing is lexed, parsed or compiled at start-up.
    if let Some(bytes) = &program.bytecode {
        return run_bundle_bytecode(bytes, &options, &anchor);
    }

    let reporter = progress::Reporter::new(None);
    run_source(
        &anchor,
        program.source,
        search_dirs,
        options,
        &[],
        &[],
        &reporter,
    )
}

/// Run a built file whose payload is compiled instructions.
///
/// Output streams as the program produces it (so an input prompt appears
/// before the read blocks) and `[run] transcript` is honored exactly as the
/// source-carrying bundle path does it: the same `<anchor>.akop`, the same
/// contents, the same closing line.
pub(crate) fn run_bundle_bytecode(bytes: &[u8], options: &RunOptions, anchor: &str) -> i32 {
    let program = match bytecode::Program::decode(bytes) {
        Ok(program) => program,
        Err(e) => {
            eprintln!("{}", e);
            if options.transcript {
                save_terminal_log(anchor, None, &e);
            }
            return 1;
        }
    };
    let mut vm = bytecode::Vm::streaming(&program);
    let result = vm.run();
    match result {
        Ok(()) => {
            // A built file never prints a timer; its compile time was shown
            // by the `akk build`/`akk run` that produced it.
            let closing_line = String::new();
            if options.transcript {
                save_terminal_log(anchor, Some(vm.log.clone()), &closing_line);
            }
            0
        }
        Err(e) => {
            eprintln!("{}", e);
            if options.transcript {
                save_terminal_log(anchor, Some(vm.log.clone()), &e);
            }
            1
        }
    }
}

/// `akk --vm <file>` -- compile the program to bytecode and run it on the
/// stack machine in `bytecode`. Output goes to stdout exactly as a normal run
/// prints it; a compile or runtime error is printed once and exits 1.
pub(crate) fn run_bytecode(file: &str, no_warn: bool) -> i32 {
    let src = match fs::read_to_string(file) {
        Ok(src) => src,
        Err(e) => {
            eprintln!("ဖိုင် \"{}\" ကို ဖတ်၍မရပါ - {}", file, e);
            return 1;
        }
    };
    // The static warnings a normal run shows first, from the same parse tree,
    // so both backends put identical text on stderr ahead of the run.
    let stmts = match lexer::lex_source(&src).and_then(|tokens| parser::parse_source(&tokens, &src))
    {
        Ok(stmts) => stmts,
        Err(e) => {
            eprintln!("{}", e);
            return 1;
        }
    };
    if !no_warn {
        let mut search_dirs = Vec::new();
        if let Some(parent) = std::path::Path::new(file).parent() {
            if !parent.as_os_str().is_empty() {
                search_dirs.push(parent.to_path_buf());
            }
        }
        for warning in
            crate::sema::check::library_warnings(libraries_dir(), &stmts, &search_dirs, &[])
        {
            eprintln!("{}", warning);
        }
    }
    // Script libraries next to the program (and in the working directory),
    // plus installed packages next to the binary: the same places the
    // interpreter's loader looks.
    let mut libraries = crate::project::library::LibraryLoader::new(libraries_dir());
    if let Some(parent) = std::path::Path::new(file).parent() {
        if !parent.as_os_str().is_empty() {
            libraries.add_script_dir(parent.to_path_buf());
        }
    }
    if let Ok(cwd) = env::current_dir() {
        libraries.add_script_dir(cwd);
    }
    match bytecode::compile_with_libraries(&stmts, &src, &mut libraries) {
        Ok(program) => {
            // Streamed, exactly like a normal run: a prompt has to be on
            // screen before the read that follows it blocks.
            let mut vm = bytecode::Vm::streaming(&program);
            match vm.run() {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("{}", e);
                    1
                }
            }
        }
        Err(e) => {
            eprintln!("{}", e);
            1
        }
    }
}

/// Run a built file, inheriting this process's stdio, and return its exit code.
pub(crate) fn run_executable(path: &Path, args: &[&str]) -> i32 {
    // A bare `one.exe` would be looked up on PATH, not in the current folder,
    // so the file is resolved to an absolute path first.
    let exe = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    match std::process::Command::new(&exe).args(args).status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(e) => {
            eprintln!("ဖိုင် \"{}\" ကို ဖွင့်၍မရပါ - {}", plain_path(path), e);
            1
        }
    }
}

/// `akk run` in a project: reuse `build/<name>` when it is current, otherwise
/// build it first. Warnings are shown by the build; the executable it then
/// runs is passed `--no-warn` so they are not printed twice.
pub(crate) fn run_compiled_project(manifest: &manifest::Manifest, no_warn: bool) -> i32 {
    if let Err(e) = manifest.validate_entry() {
        eprintln!("{}", e);
        return 1;
    }
    let out = match manifest_output_path(manifest) {
        Ok(out) => out,
        Err(e) => {
            eprintln!("{}", e);
            return 1;
        }
    };
    let extra = dependency_paths(manifest);
    let skip = manifest.root.join(&manifest.build.out_dir);
    if build_is_current(&out, &manifest.root, Some(&skip), &extra) {
        eprintln!("{}", progress::RUNNING);
        eprintln!();
        return run_executable(&out, &[]);
    }
    let code = cmd_build(no_warn);
    if code != 0 {
        return code;
    }
    eprintln!("{}", progress::RUNNING);
    eprintln!();
    run_executable(&out, &["--no-warn"])
}

/// `akk run <file>`: run the compiled file, compiling it first when it is
/// missing or stale.
///
/// The compiled file lives in a `build` folder beside the source
/// (`build/<file_name>.exe`). No `build` folder, no file, or a file older than
/// its inputs all mean the same thing: compile it, then run it. The source is
/// never interpreted directly by `akk run`.
pub(crate) fn run_compiled_file(
    file: &str,
    no_warn: bool,
    manifest: Option<&manifest::Manifest>,
) -> i32 {
    let out = single_file_cache_path(file);
    // In a project every script the build can read sits under the project
    // root, so a change anywhere there invalidates the file; without a
    // manifest only the source's own folder is scanned.
    let root = manifest.map(|m| m.root.clone()).unwrap_or_else(|| {
        Path::new(file)
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    });
    let extra = manifest.map(dependency_paths).unwrap_or_default();
    if build_is_current(&out, &root, None, &extra) {
        return run_executable(&out, &[]);
    }
    if let Err(code) = build_single_file(file, no_warn, manifest) {
        return code;
    }
    run_executable(&out, &[])
}

/// Compile one source file into `build/<file_name>.exe` beside it, creating
/// the `build` folder when it is not there yet. Bytecode is used
/// when the program can be carried that way (no imported scripts, and no
/// construct the VM refuses with `E170`); a program that needs the bytecode
/// VM's fallback -- an `App` or `request` library, a script import -- is
/// embedded as source, which still runs identically.
pub(crate) fn build_single_file(
    file: &str,
    no_warn: bool,
    manifest: Option<&manifest::Manifest>,
) -> Result<(), i32> {
    let started = std::time::Instant::now();
    let src = match fs::read_to_string(file) {
        Ok(src) => src,
        Err(e) => {
            eprintln!("ဖိုင် \"{}\" ကို ဖတ်၍မရပါ - {}", file, e);
            return Err(1);
        }
    };
    let stmts = match lexer::lex_source(&src).and_then(|tokens| parser::parse_source(&tokens, &src))
    {
        Ok(stmts) => stmts,
        Err(e) => {
            eprintln!("{}", e);
            return Err(1);
        }
    };
    let script_dir = PathBuf::from(file)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    let mut search_dirs = vec![script_dir.clone()];
    if let Ok(cwd) = env::current_dir() {
        if cwd != script_dir {
            search_dirs.push(cwd);
        }
    }
    let dependencies: Vec<(String, PathBuf)> = manifest
        .map(|m| dependency_pairs(m.resolve_dependencies(&libraries_dir()).unwrap_or_default()))
        .unwrap_or_default();
    let embed_root = manifest
        .map(|m| m.root.clone())
        .unwrap_or_else(|| script_dir.clone());
    let embedded = match collect_embedded(&stmts, &embed_root, &search_dirs, &dependencies) {
        Ok(scripts) => scripts,
        Err(e) => {
            eprintln!("{}", e);
            return Err(1);
        }
    };
    let options = manifest
        .map(|m| RunOptions::from_manifest(&m.run, no_warn))
        .unwrap_or_else(|| RunOptions::defaults(no_warn));

    let has_deps: u32 = if embedded.is_empty() { 0 } else { 0b0001 };
    let transcript_bit: u32 = if options.transcript { 0b0010 } else { 0 };
    let warnings_bit: u32 = if options.warnings { 0b0100 } else { 0 };
    let run_flags = has_deps | transcript_bit | warnings_bit;

    let wants_bytecode = manifest
        .map(|m| matches!(m.build.mode, manifest::BuildMode::Bytecode))
        .unwrap_or(true);
    let compiled = if wants_bytecode && embedded.is_empty() {
        match bytecode::compile_with_source(&stmts, &src).and_then(|program| program.encode()) {
            Ok(bytes) => Some(bytes),
            // The VM cannot carry this program (a native library, a library
            // function value). Source embedding runs it exactly as `akk` does.
            Err(e) if e.contains("E170") => None,
            Err(e) => {
                eprintln!("{}", e);
                return Err(1);
            }
        }
    } else {
        None
    };

    let out = single_file_cache_path(file);
    if let Some(dir) = out.parent() {
        if let Err(e) = fs::create_dir_all(dir) {
            eprintln!("ဖိုဒါ \"{}\" ကို ဖန်တီး၍မရပါ - {}", plain_path(dir), e);
            return Err(1);
        }
    }
    let input = bundle::BuildInput {
        source: &src,
        bytecode: compiled.as_deref(),
        deps: &embedded,
        run_flags,
    };
    match bundle::write_bundle(&out, &input) {
        Ok(_) => {}
        Err(e) => {
            eprintln!("{}", e);
            return Err(1);
        }
    }
    progress::compiled_in(started.elapsed().as_secs_f64());
    Ok(())
}

/// Run one parsed program on a thread with a large stack. A tree-walking
/// interpreter uses several stack frames per Akkhara call, and the main
/// thread's default stack (1 MB on Windows) overflowed at roughly 200
/// recursive calls. 64 MB carries the frame budget `MAX_CALL_DEPTH` allows,
/// and the interpreter's own limit (E135) stops a runaway recursion with a
/// catchable error before the stack runs out. `silent` keeps the program's
/// output in the transcript instead of writing it to the terminal, which is
/// what `akk report` needs to print each program under its own heading, and
/// `warnings_off` (`--no-warn`) drops the warnings the run itself would print.
///
/// `Err` carries the message to show and the exit code the run would have
/// ended with: 1 when the interpreter thread could not start, 101 when it
/// panicked. `source` is the program's own text, so a runtime error can be
/// shown with the line it names, the same way a syntax error is.
#[allow(clippy::result_large_err)]
pub(crate) fn interpret(
    stmts: Vec<parser::Stmt>,
    source: String,
    libraries_dir: PathBuf,
    search_dirs: Vec<PathBuf>,
    dependencies: Vec<(String, PathBuf)>,
    silent: bool,
    warnings_off: bool,
) -> Result<RunOutcome, (String, i32)> {
    let run = std::thread::Builder::new()
        .name("akk-interpreter".to_string())
        .stack_size(64 * 1024 * 1024)
        .spawn(move || -> RunOutcome {
            let mut interp = interpreter::Interpreter::new(libraries_dir);
            for dir in search_dirs {
                interp.add_library_search_dir(dir);
            }
            for (name, path) in dependencies {
                interp.add_manifest_dependency(&name, path);
            }
            // Collect the program's output so it can be saved alongside the
            // program (in addition to being shown on stdout).
            interp.capture_output();
            if silent {
                interp.silence_output();
            }
            if warnings_off {
                interp.suppress_warnings();
            }

            // Time the interpretation itself (not the lex/parse step) and
            // report it once the program has finished successfully.
            let started = std::time::Instant::now();
            let result = interp.run_with_source(&stmts, &source);
            // Output is line-flushed as the program runs; this only gets out
            // a last line that has no newline of its own, before the error
            // that may follow it.
            interp.flush_output();
            let elapsed = started.elapsed().as_secs_f64();
            let output = interp.take_output();
            RunOutcome {
                elapsed,
                output,
                error: result.err(),
            }
        });

    let outcome = match run {
        Ok(handle) => handle
            .join()
            .map_err(|_| ("akk: interpreter thread ပျက်ကျသွားပါသည်".to_string(), 101i32))?,
        Err(e) => return Err((format!("akk: interpreter thread ကို စတင်၍မရပါ - {}", e), 1)),
    };
    Ok(outcome)
}

/// Show the static warnings on stderr and hand back the lines they contribute
/// to the run's `.akop` transcript, so a saved transcript holds everything the
/// terminal showed. Warnings are printed before the program runs.
pub(crate) fn write_warnings(warnings: &[String]) -> String {
    let mut text = String::new();
    for warning in warnings {
        eprintln!("{}", warning);
        text.push_str(warning);
        text.push('\n');
    }
    text
}

/// The transcript `save_terminal_log` writes: the static warnings first (they
/// are shown before the program starts), then the program's own output.
pub(crate) fn with_warnings(prefix: &str, output: Option<String>) -> Option<String> {
    if prefix.is_empty() {
        return output;
    }
    Some(format!("{}{}", prefix, output.unwrap_or_default()))
}

/// What the interpreter thread produced: the timed section's duration, the
/// output captured for the `.akop` transcript, and the error (if any).
pub(crate) struct RunOutcome {
    elapsed: f64,
    output: Option<String>,
    error: Option<String>,
}

/// `akk report [folder]` -- runs every `.akk` program in `folder` (default:
/// `output`) and prints, one heading per program, what each one produced: its
/// warnings, its error when it failed, and its own output -- then a count of
/// how many succeeded and failed. The programs run with their output captured,
/// so everything stays under its own heading; `--no-warn` silences the static
/// warnings. Exits 0 when every program succeeded and 1 when one did not.
pub(crate) fn cmd_report(folder: &str, no_warn: bool) {
    let dir = PathBuf::from(folder);
    let mut files: Vec<PathBuf> = match fs::read_dir(&dir) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "akk"))
            .collect(),
        Err(e) => {
            eprintln!("ဖိုင် \"{}\" ကို ဖတ်၍မရပါ - {}", dir.display(), e);
            process::exit(1);
        }
    };
    files.sort();

    println!("==> {} ထဲရှိ .akk ပရိုဂရမ် {} ခု", dir.display(), files.len());

    let mut successes = 0usize;
    let mut failures = 0usize;
    let mut warning_count = 0usize;

    for file in &files {
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| file.display().to_string());
        println!();
        println!("--- {} ---", name);

        let src = match fs::read_to_string(file) {
            Ok(src) => src,
            Err(e) => {
                println!("[ERROR] သတိပေးချက် 0၊ အမှား 1");
                println!("    {}", e);
                failures += 1;
                continue;
            }
        };

        // Parse, check, run -- the same order and the same checks a plain
        // `akk <file>` does, so the report sees exactly what a run shows.
        let stmts =
            match lexer::lex_source(&src).and_then(|tokens| parser::parse_source(&tokens, &src)) {
                Ok(stmts) => stmts,
                Err(e) => {
                    println!("[ERROR] သတိပေးချက် 0၊ အမှား 1");
                    println!("    {}", e);
                    failures += 1;
                    continue;
                }
            };
        let mut search_dirs = Vec::new();
        if let Some(parent) = file.parent() {
            search_dirs.push(parent.to_path_buf());
        }
        let found_warnings: Vec<String> = if no_warn {
            Vec::new()
        } else {
            crate::sema::check::library_warnings(libraries_dir(), &stmts, &search_dirs, &[])
        };
        let outcome = match interpret(
            stmts,
            src,
            libraries_dir(),
            search_dirs,
            Vec::new(),
            true,
            no_warn,
        ) {
            Ok(outcome) => outcome,
            Err((message, _)) => {
                println!("[ERROR] သတိပေးချက် {}၊ အမှား 1", found_warnings.len());
                println!("    {}", message);
                failures += 1;
                continue;
            }
        };

        // The heading comes first and the lines under it are its details:
        // the static warnings, then whatever the program itself printed (a
        // runtime warning such as E046 sits where it was printed), then the
        // error that ended a failing run.
        if outcome.error.is_some() {
            println!(
                "[ERROR] သတိပေးချက် {}၊ အမှား 1၊ run {:.3}s",
                found_warnings.len(),
                outcome.elapsed
            );
            failures += 1;
        } else {
            println!(
                "[SUCCESS] သတိပေးချက် {}၊ အမှား 0၊ run {:.3}s",
                found_warnings.len(),
                outcome.elapsed
            );
            successes += 1;
        }
        for warning in &found_warnings {
            println!("{}", warning);
        }
        warning_count += found_warnings.len();
        if let Some(output) = &outcome.output {
            print!("{}", output);
        }
        if let Some(error) = &outcome.error {
            println!("{}", error);
        }
    }

    println!();
    println!(
        "==> စုစုပေါင်း - အောင်မြင် {} ခု၊ အမှား {} ခု၊ သတိပေးချက် {} ခု",
        successes, failures, warning_count
    );
    if failures > 0 {
        process::exit(1);
    }
}

/// `akk ir [--opt] <file>`: print the symbol table, HIR and MIR the middle end
/// builds. `--opt` adds the MIR again after constant folding and dead-code
/// elimination, so the two sections can be compared. Nothing is executed, and
/// no file is written. A construct MIR cannot represent is printed as its
/// `E176` message after the HIR, and the exit code says the dump is partial.
pub(crate) fn cmd_ir(file: &str, optimize: bool) -> i32 {
    let options = crate::ir::Options { optimize };
    match crate::ir::dump_file(file, options) {
        Ok(dump) => {
            print!("{}", dump.text);
            if dump.refused.is_some() {
                1
            } else {
                0
            }
        }
        Err(message) => {
            eprintln!("{}", message);
            1
        }
    }
}
