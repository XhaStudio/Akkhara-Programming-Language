mod bundle;
mod bytecode;
mod interpreter;
mod lexer;
mod library;
mod manifest;
mod parser;
mod progress;
mod text;
mod ui;
mod warnings;

/// The Akkhara "အချိန်" (Time) library, compiled in from its own source
/// under `libraries/အချိန်/main.rs`. Programs load it with:
///     နည်းပညာများ အချိန် ကို သုံးမည်။
#[path = "../libraries/အချိန်/main.rs"]
mod time_library;

/// The Akkhara "ကျပန်း" (Random) library, compiled in from its own source
/// under `libraries/ကျပန်း/main.rs`. Programs load it with:
///     နည်းပညာများ ကျပန်း ကို သုံးမည်။
#[path = "../libraries/ကျပန်း/main.rs"]
mod random_library;

/// The Akkhara "request" library, compiled in from its own source under
/// `libraries/request/main.rs`. Programs load it with:
///     နည်းပညာများ request ကို သုံးမည်။
#[path = "../libraries/request/main.rs"]
mod request_library;

/// The Akkhara "App" GUI library, compiled in from its own source under
/// `libraries/App/main.rs`. Programs load it with:
///     နည်းပညာများ App ကို သုံးမည်။
#[path = "../libraries/App/main.rs"]
mod app_library;

/// The Akkhara "server" backend library, compiled in from its own source under
/// `libraries/server/main.rs`. It answers to two names -- `server` and `ဆာဗာ` --
/// and every function has a spelling in each language. Programs load it with
/// `use server;` or:
///     နည်းပညာများ ဆာဗာ ကို သုံးမည်။
#[path = "../libraries/server/main.rs"]
mod server_library;

/// The Akkhara "json" library, compiled in from its own source under
/// `libraries/json/main.rs`. It answers to two names -- `json` and `ဂျေဆန်` --
/// and every function has a spelling in each language. Programs load it with
/// `use json;` or:
///     နည်းပညာများ ဂျေဆန် ကို သုံးမည်။
#[path = "../libraries/json/main.rs"]
mod json_library;

/// The Akkhara "file" library, compiled in from its own source under
/// `libraries/file/main.rs`. It answers to two names -- `file` and `ဖိုင်` --
/// and every function has a spelling in each language. Programs load it with
/// `use file;` or:
///     နည်းပညာများ ဖိုင် ကို သုံးမည်။
#[path = "../libraries/file/main.rs"]
mod file_library;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{self, Command};

/// Pulled from Cargo.toml at build time (`[package] version = "..."`).
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// GitHub repo the installer / updater pulls releases from.
const REPO: &str = "XhaStudio/Akkhara-Programming-Language";

/// GitHub `owner/repo` that hosts installable Akkhara library packages.
const LIBRARY_REPO: &str = "XhaStudio/Akkhara-Libraries";

/// Path, within `LIBRARY_REPO`'s `main` branch, to the folder holding all
/// published libraries. Each library is a folder under
/// `<LIBRARY_PATH>/<name>/`: `main/` holds the library's source file(s)
/// (an entry `main.akk`, plus any others it needs), and `index.json` is
/// its manifest (name/version/description/entry). `<LIBRARY_PATH>/index.json`
/// lists every published library name, and is what `akk install` browses
/// when it can't find an exact match.
const LIBRARY_PATH: &str = "Libraries";

/// Base raw-content URL for `LIBRARY_PATH` on `LIBRARY_REPO`'s `main`
/// branch, e.g. `.../Libraries/<name>/index.json`.
fn library_raw_base() -> String {
    format!(
        "https://raw.githubusercontent.com/{}/main/{}",
        LIBRARY_REPO, LIBRARY_PATH
    )
}

/// Where `akk install` places downloaded packages, and where
/// `နည်းပညာများ <name> ကို သုံးမည်။` looks for them at runtime: a
/// `libraries/` folder next to the akk binary itself (not the current
/// working directory), so it works no matter where a script is run from.
fn libraries_dir() -> PathBuf {
    env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("libraries")))
        .unwrap_or_else(|| PathBuf::from("libraries"))
}

/// Saves everything the run showed on the terminal next to the program as
/// `<file name>.akop`: the output collected while the program ran, followed
/// by the run's closing line -- the error message when it fails, or the
/// `Interpreted in ...` line when it succeeds. It is written on every run, so
/// a failing program leaves its `.akop` behind as well.
fn save_terminal_log(filename: &str, output: Option<String>, closing_line: &str) {
    // Appended as-is, without forcing a line break: a program that ends in a
    // prompt (printed without a trailing newline) shows its closing line on
    // the same terminal line, and the log keeps that text.
    let mut log = output.unwrap_or_default();
    log.push_str(closing_line);
    log.push('\n');
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

fn print_usage() {
    eprintln!("Usage: akk <file name> [--no-warn]");
    eprintln!("       akk run [file]             (akk.toml ပရောဂျက်ကို ဖွင့်သည်)");
    eprintln!("       akk new <name>             (ပရောဂျက် ဖိုဒါ အသစ် ဖန်တီးသည်)");
    eprintln!("       akk build                  (ပရောဂျက်ကို ဖွင့်ပေးမည့် executable တစ်ခု ရေးသည်)");
    eprintln!("       akk report [folder]        (ဖိုဒါထဲရှိ .akk အားလုံးကို ဖွင့်သည်)");
    eprintln!("       akk --version | -v");
    eprintln!("       akk --check");
    eprintln!("       akk --vm <file name>       (bytecode စက်ဖြင့် ဖွင့်သည်)");
    eprintln!("       akk install [library]      (အမည် မပါလျှင် akk.toml မှီခိုချက်များကို install လုပ်သည်)");
    eprintln!("       akk update [version]");
    eprintln!("       akk uninstall");
}

fn main() {
    // A built file (`akk build`) is this same interpreter with one program
    // appended: it runs that program and nothing else. This must be the first
    // statement, before the libraries/ folder is created, before an argument
    // is looked at and before any dispatch. A plain `akk` reads its own slot
    // from memory and carries on unchanged.
    if let Some(bundle) = bundle::probe() {
        process::exit(run_bundle(bundle));
    }

    // Make sure the libraries/ folder exists next to the binary. Older
    // installs (and fresh downloads of a release tarball/zip) don't ship
    // this folder, so create it lazily on every run rather than relying on
    // the install scripts alone.
    let _ = fs::create_dir_all(libraries_dir());

    // `--no-warn` turns the static warnings off (see `warnings`). It is
    // accepted before or after the file name / subcommand, so it is taken out
    // of the argument list here rather than parsed per command.
    let args: Vec<String> = env::args().collect();
    let no_warn = args.iter().any(|a| a == "--no-warn");
    // `--vm` runs a program on the bytecode compiler instead of the tree
    // walker; it is taken out of the argument list like `--no-warn`.
    let bytecode_mode = args.iter().any(|a| a == "--vm");
    let args: Vec<String> = args
        .into_iter()
        .filter(|a| a != "--no-warn" && a != "--vm")
        .collect();

    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    match args[1].as_str() {
        "--version" | "-v" | "version" => {
            println!("akk {}", VERSION);
            return;
        }
        "--help" | "-h" | "help" => {
            print_usage();
            return;
        }
        "--check" | "check" => {
            cmd_check();
            return;
        }
        _ if bytecode_mode => {
            // `akk --vm <file>` -- compile to bytecode and run it.
            process::exit(run_bytecode(&args[1]));
        }
        "report" => {
            let folder = args.get(2).map(String::as_str).unwrap_or("output");
            cmd_report(folder, no_warn);
            return;
        }
        "install" => {
            // With no name, install everything the project's manifest lists;
            // with one, install exactly that library (unchanged behavior).
            if args.len() < 3 {
                cmd_install_manifest();
                return;
            }
            cmd_install(&args[2]);
            return;
        }
        "new" => {
            // Creates `{name}/src/main.akk` and its manifest; see `cmd_new`.
            process::exit(cmd_new(args.get(2).map(String::as_str)));
        }
        "build" => {
            process::exit(cmd_build(no_warn));
        }
        "run" => {
            process::exit(cmd_run(no_warn, args.get(2).map(String::as_str)));
        }
        "update" => {
            let version = args.get(2).map(|s| s.as_str());
            cmd_update(version);
            return;
        }
        "uninstall" => {
            cmd_uninstall();
            return;
        }
        _ => {}
    }

    // A plain `akk file.akk` never looks for a manifest: its behavior is
    // exactly what it has always been. `akk run` is the manifest-aware entry
    // point (see `cmd_run`).
    process::exit(run_program(
        &args[1],
        RunOptions::defaults(no_warn),
        &[],
        &[],
        None,
    ));
}

/// What a run shows and writes. `akk file.akk` gets every setting on; `akk
/// run` takes them from the manifest's `[run]` table.
#[derive(Debug, Clone, Copy)]
struct RunOptions {
    /// Write the `<file>.akop` transcript (including after a failure).
    transcript: bool,
    /// Print the closing `Interpreted in ...` line on success.
    timing: bool,
    /// Show the static and runtime warnings.
    warnings: bool,
}

impl RunOptions {
    /// The historical behavior of `akk file.akk` (`--no-warn` turns only the
    /// warnings off).
    fn defaults(no_warn: bool) -> Self {
        RunOptions {
            transcript: true,
            timing: true,
            warnings: !no_warn,
        }
    }

    /// A manifest's `[run]` settings, with `--no-warn` still winning.
    fn from_manifest(run: &manifest::RunSettings, no_warn: bool) -> Self {
        RunOptions {
            transcript: run.transcript,
            timing: run.timing,
            warnings: run.warnings && !no_warn,
        }
    }

    /// The settings a built file baked in at `akk build` time (the header
    /// flags in `bundle`); `--no-warn` still wins on the command line.
    fn from_flags(flags: u32) -> Self {
        RunOptions {
            transcript: flags & 0b0010 != 0,
            timing: flags & 0b1000 != 0,
            warnings: flags & 0b0100 != 0,
        }
    }
}

/// Run one program file, honoring `options`, and return the exit code the
/// process should end with. This is the whole plain run path: `akk file.akk`
/// and `akk run` both come through here, so the two behave identically apart
/// from the settings, the extra manifest warnings and `project` -- the header
/// lines only a project run prints.
fn run_program(
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
fn run_source(
    anchor: &str,
    src: String,
    search_dirs: Vec<PathBuf>,
    options: RunOptions,
    extra_warnings: &[String],
    dependencies: &[(String, PathBuf)],
    reporter: &progress::Reporter,
) -> i32 {
    let libs_dir = libraries_dir();

    let tokens = match lexer::lex(&src) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{}", e);
            if options.transcript {
                save_terminal_log(anchor, None, &e);
            }
            return 1;
        }
    };

    let stmts = match parser::parse(&tokens) {
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
        all.extend(library_warnings(&stmts, &search_dirs, dependencies));
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

    let closing_line = if options.timing {
        let line = format!("Interpreted in {:.3}s", outcome.elapsed);
        println!("{}", line);
        line
    } else {
        // `[run] timing = false` keeps the closing line out of the terminal.
        // The transcript still ends with its own newline (see
        // `save_terminal_log`).
        String::new()
    };

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
fn cmd_run(no_warn: bool, file: Option<&str>) -> i32 {
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
fn run_bundle(program: bundle::Bundle) -> i32 {
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
fn run_bundle_bytecode(bytes: &[u8], options: &RunOptions, anchor: &str) -> i32 {
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
    let started = std::time::Instant::now();
    let mut vm = bytecode::Vm::streaming(&program);
    let result = vm.run();
    match result {
        Ok(()) => {
            let closing_line = if options.timing {
                let line = format!("Interpreted in {:.3}s", started.elapsed().as_secs_f64());
                println!("{}", line);
                line
            } else {
                String::new()
            };
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
fn run_bytecode(file: &str) -> i32 {
    let src = match fs::read_to_string(file) {
        Ok(src) => src,
        Err(e) => {
            eprintln!("ဖိုင် \"{}\" ကို ဖတ်၍မရပါ - {}", file, e);
            return 1;
        }
    };
    match bytecode::compile_source(&src) {
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

/// `akk build` -- write one executable that runs this project.
///
/// The output is a copy of this interpreter with the program's source, and the
/// scripts it brings in, appended after it. It is not machine code and it does
/// not run faster; a built file only saves typing `akk run`. Building never
/// runs the program and writes no `.akop`.
fn cmd_build(no_warn: bool) -> i32 {
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

    let mut name = manifest.build.output.clone();
    if cfg!(windows) {
        if let Some(windows) = &manifest.windows_output {
            if !windows.is_empty() {
                name = windows.clone();
            }
        }
    }
    if let Err(e) = check_output_name(&name) {
        eprintln!("{}", e);
        return 1;
    }
    let suffix = std::env::consts::EXE_SUFFIX;
    if !suffix.is_empty() && !name.ends_with(suffix) {
        name.push_str(suffix);
    }
    let out = manifest.root.join(&manifest.build.out_dir).join(&name);

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

    let filename = manifest.entry_path().display().to_string();
    let src = match fs::read_to_string(&filename) {
        Ok(src) => src,
        Err(e) => {
            eprintln!("ဖိုင် \"{}\" ကို ဖတ်၍မရပါ - {}", filename, e);
            return 1;
        }
    };
    let tokens = match lexer::lex(&src) {
        Ok(tokens) => tokens,
        Err(e) => {
            eprintln!("{}", e);
            return 1;
        }
    };
    let stmts = match parser::parse(&tokens) {
        Ok(stmts) => stmts,
        Err(e) => {
            eprintln!("{}", e);
            return 1;
        }
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
        Err(e) => {
            eprintln!("{}", e);
            return 1;
        }
    };
    // Dependencies first: a name that is both a path dependency and a
    // neighbouring script resolves to the dependency at run time.
    embedded.sort_by_key(|script| script.kind);

    let static_warnings = if no_warn || !manifest.run.warnings {
        Vec::new()
    } else {
        let mut all = manifest.warnings.clone();
        all.extend(library_warnings(&stmts, &search_dirs, &dependencies));
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
            eprintln!(
                "E170 [build] mode = \"bytecode\" သည် နည်းပညာ (library) များကို မထောက်ပံ့သေးပါ။ \"bundle\" ကို သုံးပါ။"
            );
            return 1;
        }
        match bytecode::compile(&stmts).and_then(|program| program.encode()) {
            Ok(bytes) => Some(bytes),
            Err(e) => {
                eprintln!("{}", e);
                return 1;
            }
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

    reporter.finished(&plain_path(&out), bytes);
    0
}

/// `[build] output` / `[target.windows] output` must be one file name: no
/// folder, no `..`, not empty. `out_dir` is the only way to choose a folder.
fn check_output_name(name: &str) -> Result<(), String> {
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
fn collect_embedded(
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

fn push_uses(stmts: &[parser::Stmt], queue: &mut Vec<String>) {
    for stmt in stmts {
        if let parser::Stmt::UseLibrary { libs, .. } = stmt {
            for (name, _alias) in libs {
                queue.push(name.clone());
            }
        }
    }
}

/// The folder ignore file `akk new` writes: the default `[build] out_dir` and
/// the `.akop` transcripts `save_terminal_log` leaves next to a program.
const NEW_GITIGNORE: &str = "/build/\n*.akop\n";

/// A path as the user should see it: no Windows verbatim (`\\?\`) prefix,
/// which `canonicalize` and some shells add.
fn plain_path(path: &Path) -> String {
    let text = path.display().to_string();
    match text.strip_prefix(r"\\?\") {
        Some(rest) => rest.to_string(),
        None => text,
    }
}

/// A project name `akk new` accepts: one folder component that is safe on
/// every platform we ship. The rule is a blacklist plus a few structural
/// checks -- deliberately *not* an alphanumeric whitelist, because Myanmar
/// letters and combining marks (e.g. U+1039) must stay usable. The limit is
/// 255 bytes: one Myanmar letter is 3 bytes, so a smaller cap would refuse
/// ordinary names.
fn valid_project_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 255 {
        return false;
    }
    if name == "." || name == ".." || name.starts_with('-') {
        return false;
    }
    // Reserved device names, with or without an extension (`CON`, `con.txt`).
    let stem = name.split('.').next().unwrap_or(name);
    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
        "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem)) {
        return false;
    }
    name.chars().all(|c| {
        !c.is_whitespace()
            && !c.is_control()
            && !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
    })
}

/// Create the folder tree and write the three files. Split out so `cmd_new`
/// can delete the folder when any step fails: a half-written project would
/// make the next `akk new <name>` fail with E166.
fn create_project(root: &Path, manifest_text: &str, program_text: &str) -> std::io::Result<()> {
    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir)?;
    fs::write(root.join("akk.toml"), manifest_text)?;
    fs::write(root.join(".gitignore"), NEW_GITIGNORE)?;
    fs::write(src_dir.join("main.akk"), program_text)?;
    Ok(())
}

/// `akk new <name>` -- create `<name>/` with a manifest, an ignore file and a
/// first program, then print how to enter it. Nothing outside the new folder
/// is touched, and no `git` command runs.
///
/// `args.get(2)` hands an explicitly empty argument through as `Some("")`; an
/// empty name cannot become a folder, so it is treated like a missing name and
/// prints the usage line.
fn cmd_new(name: Option<&str>) -> i32 {
    let Some(name) = name.filter(|n| !n.is_empty()) else {
        eprintln!("Usage: akk new <name>");
        return 1;
    };

    if !valid_project_name(name) {
        eprintln!(
            "E167 akk new: \"{}\" ကို ပရောဂျက် အမည်အဖြစ် မသုံးနိုင်ပါ။ အမည်တွင် ကွက်လပ်၊ / \\ : * ? \" < > | နှင့် control character မပါရပါ။",
            name
        );
        return 1;
    }

    let root = PathBuf::from(name);
    // `symlink_metadata` also succeeds for a dangling symlink, which `exists()`
    // reports as absent: creating into it would follow the link, and the
    // cleanup below would then delete a path this command never created.
    if root.symlink_metadata().is_ok() {
        eprintln!(
            "E166 akk new: \"{}\" သည် ရှိပြီးဖြစ်ပါသည်။ အခြားအမည်ပေးပါ။",
            name
        );
        return 1;
    }

    // Every key below is read by `manifest::load` (`PACKAGE_KEYS`,
    // `BUILD_KEYS`, `RUN_KEYS`), so the generated file raises no W008.
    let manifest_text = format!(
        "[package]\nname = \"{}\"\nversion = \"0.1.0\"\nentry = \"src/main.akk\"\n\n[build]\nout_dir = \"build\"\n\n[run]\ntranscript = true\nwarnings = true\ntiming = true\n",
        name
    );
    let program_text = format!("# {}\n\"မင်္ဂလာပါ!\" ကို ဖော်ပြပါ။\n", name);

    if let Err(e) = create_project(&root, &manifest_text, &program_text) {
        let _ = fs::remove_dir_all(&root);
        eprintln!("akk new: ဖိုဒါ ဖန်တီး၍မရပါ - {}", e);
        return 1;
    }

    println!(
        "akk new: \"{}\" ဖိုဒါကို ဖန်တီးပြီးပါပြီ။ \"cd {}\" ဖြင့် ဝင်ပါ။",
        name, name
    );
    0
}

/// The `(name, path)` pairs the interpreter registers for `[dependencies]`
/// entries that resolved to a source file. Built-ins have no file to load.
fn dependency_pairs(resolved: Vec<manifest::ResolvedDependency>) -> Vec<(String, PathBuf)> {
    resolved
        .into_iter()
        .filter_map(|dependency| dependency.path.map(|path| (dependency.name, path)))
        .collect()
}

/// `akk install` with no library name -- installs every `[dependencies]`
/// entry the project's manifest lists. `path` dependencies are already in the
/// project, so only version/git entries are downloaded; each one reuses
/// `cmd_install`.
fn cmd_install_manifest() {
    let cwd = match env::current_dir() {
        Ok(cwd) => cwd,
        Err(e) => {
            eprintln!("E146 akk.toml ကို ရှာရာတွင် အမှား ဖြစ်ပေါ်ခဲ့ပါသည် - {}", e);
            process::exit(1);
        }
    };
    let Some(path) = manifest::discover(&cwd) else {
        eprintln!(
            "E146 ဤ ဖိုဒါတွင် akk.toml ကို ရှာမတွေ့ပါ။ \"akk install <library>\" ဖြင့် နည်းပညာ တစ်ခုကို ထည့်သွင်းပါ။"
        );
        process::exit(1);
    };
    let manifest = match manifest::load(&path) {
        Ok(manifest) => manifest,
        Err(e) => {
            eprintln!("{}", e);
            process::exit(1);
        }
    };

    let names: Vec<String> = manifest
        .dependencies
        .iter()
        .filter(|dependency| !matches!(dependency.source, manifest::DependencySource::Path(_)))
        .map(|dependency| dependency.name.clone())
        .collect();

    if names.is_empty() {
        println!("==> akk.toml ၏ [dependencies] တွင် install လုပ်ရန် နည်းပညာ မရှိပါ။");
        return;
    }

    println!("==> akk.toml မှီခိုချက် {} ခုကို install လုပ်နေသည်...", names.len());
    for name in names {
        cmd_install(&name);
    }
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
/// panicked.
#[allow(clippy::result_large_err)]
fn interpret(
    stmts: Vec<parser::Stmt>,
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
            let result = interp.run(&stmts);
            // Output is line-flushed as the program runs; this only gets out
            // a last line that has no newline of its own, before the closing
            // line (or the error) that follows it. This stays inside the
            // timed section, so `Interpreted in ...` still covers the cost of
            // writing it.
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
        Err(e) => {
            return Err((
                format!("akk: interpreter thread ကို စတင်၍မရပါ - {}", e),
                1,
            ))
        }
    };
    Ok(outcome)
}

/// Check a program and the script/package libraries it imports, resolving an
/// import the way a run does (a downloaded package first, then a plain
/// `<name>.akk` script in one of `search_dirs`). A built-in library and a name
/// that is not on disk add nothing: there is no source to read.
fn library_warnings(
    stmts: &[parser::Stmt],
    search_dirs: &[PathBuf],
    dependencies: &[(String, PathBuf)],
) -> Vec<String> {
    let mut loader = library::LibraryLoader::new(libraries_dir());
    for dir in search_dirs {
        loader.add_script_dir(dir.clone());
    }
    for (name, path) in dependencies {
        loader.add_dependency(name, path.clone());
    }
    warnings::check_program(stmts, &|name| {
        loader
            .find_source(name)
            .map(|(path, source)| warnings::Library {
                file: path.display().to_string(),
                source,
            })
    })
}

/// Show the static warnings on stderr and hand back the lines they contribute
/// to the run's `.akop` transcript, so a saved transcript holds everything the
/// terminal showed. Warnings are printed before the program runs.
fn write_warnings(warnings: &[String]) -> String {
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
fn with_warnings(prefix: &str, output: Option<String>) -> Option<String> {
    if prefix.is_empty() {
        return output;
    }
    Some(format!("{}{}", prefix, output.unwrap_or_default()))
}

/// What the interpreter thread produced: the timed section's duration, the
/// output captured for the `.akop` transcript, and the error (if any).
struct RunOutcome {
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
fn cmd_report(folder: &str, no_warn: bool) {
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
        let stmts = match lexer::lex(&src).and_then(|tokens| parser::parse(&tokens)) {
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
            library_warnings(&stmts, &search_dirs, &[])
        };
        let outcome = match interpret(
            stmts,
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

/// `akk --check` -- looks up the latest GitHub release tag and compares it
/// against the version this binary was built with. Read-only: it never
/// downloads or installs anything, unlike `akk update`.
fn cmd_check() {
    println!("==> အပ်ဒိတ် ရှိမရှိ စစ်ဆေးနေသည်...");

    let api_url = format!("https://api.github.com/repos/{}/releases/latest", REPO);

    #[cfg(target_os = "windows")]
    let fetch_result = {
        let ps_cmd = format!(
            "(Invoke-RestMethod -Uri '{}' -Headers @{{ 'User-Agent' = 'akk-cli' }}).tag_name",
            api_url
        );
        Command::new("powershell")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                &ps_cmd,
            ])
            .output()
    };

    // Note: -f (fail-on-HTTP-error) is intentionally left off here, unlike
    // in update/install -- we want the response body (which carries GitHub's
    // actual error message, e.g. rate limiting) even on a non-2xx status.
    #[cfg(not(target_os = "windows"))]
    let fetch_result = Command::new("curl")
        .args(["-sSL", "-H", "User-Agent: akk-cli", &api_url])
        .output();

    let output = match fetch_result {
        Ok(o) => o,
        Err(e) => {
            eprintln!("    [FAILED] အပ်ဒိတ်ကို စစ်ဆေး၍မရပါ - {}", e);
            eprintln!(
                "    curl (သို့မဟုတ် PowerShell) ရရှိနိုင်ပြီး ကွန်ရက် ချိတ်ဆက်မှု ရှိကြောင်း စစ်ပါ။"
            );
            process::exit(1);
        }
    };

    let body = String::from_utf8_lossy(&output.stdout);

    // On Windows, PowerShell already prints just the bare tag_name string.
    // On Unix, we get the full release JSON and need to pull tag_name out
    // ourselves -- kept dependency-free, same approach install.sh uses.
    #[cfg(target_os = "windows")]
    let latest_tag = body.trim().trim_matches('"').to_string();

    #[cfg(not(target_os = "windows"))]
    let latest_tag = match extract_json_string_field(&body, "tag_name") {
        Some(t) => t,
        None => {
            eprintln!("    [FAILED] GitHub မှ နောက်ဆုံး release အချက်အလက်ကို ဖတ်၍မရပါ");
            if let Some(msg) = extract_json_string_field(&body, "message") {
                eprintln!("    GitHub ၏ အချက် - {}", msg);
            }
            process::exit(1);
        }
    };

    if latest_tag.is_empty() {
        eprintln!("    [FAILED] GitHub မှ {} အတွက် release အချက်အလက် မပြန်လာပါ", REPO);
        process::exit(1);
    }

    let latest_version = latest_tag.trim_start_matches('v');

    println!("    တပ်ဆင်ထားသည် - {}", VERSION);
    println!("    နောက်ဆုံး -    {}", latest_version);
    println!();

    if latest_version == VERSION {
        println!("    [OK] နောက်ဆုံး ဗားရှင်း ဖြစ်ပါသည်။");
    } else {
        println!("    [!] အခြား ဗားရှင်း တစ်ခု ရနိုင်ပါသည်။");
        println!("        တပ်ဆင်ရန် 'akk update' ကို ဖွင့်ပါ။");
    }
}

/// Splits a top-level JSON array of objects (`[{...}, {...}]`) into the
/// raw text of each object, so per-item fields can be pulled out with
/// `extract_json_string_field`. Used to walk the GitHub contents API's
/// directory-listing response. Braces and quotes inside string literals
/// are skipped so they don't confuse the split; not a general parser.
fn split_json_objects(body: &str) -> Vec<&str> {
    let mut objects = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    let mut in_string = false;
    let mut escape = false;
    for (i, c) in body.char_indices() {
        if in_string {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => {
                if depth == 0 {
                    start = i;
                }
                depth += 1;
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    objects.push(&body[start..=i]);
                }
            }
            _ => {}
        }
    }
    objects
}

/// Pulls `"field": "value"` out of a flat JSON body without a JSON crate.
/// Good enough for GitHub's release/contents API response shapes and
/// akk's own index.json manifests; not a general parser.
pub(crate) fn extract_json_string_field(body: &str, field: &str) -> Option<String> {
    let needle = format!("\"{}\":", field);
    let start = body.find(&needle)? + needle.len();
    let rest = body[start..].trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Pulls the quoted string entries out of a `"field": ["a", "b", ...]`
/// JSON array. Used to read the package name list out of index.json.
fn extract_json_string_array(body: &str, field: &str) -> Vec<String> {
    let needle = format!("\"{}\":", field);
    let Some(start) = body.find(&needle) else {
        return Vec::new();
    };
    let rest = &body[start + needle.len()..];
    let Some(open) = rest.find('[') else {
        return Vec::new();
    };
    let Some(close) = rest[open..].find(']') else {
        return Vec::new();
    };
    let array_body = &rest[open + 1..open + close];

    let mut names = Vec::new();
    let mut chars = array_body.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c == '"' {
            if let Some(end_offset) = array_body[i + 1..].find('"') {
                names.push(array_body[i + 1..i + 1 + end_offset].to_string());
            }
        }
    }
    names
}

/// `akk update [version]` -- re-runs the official installer, which fetches
/// either the latest release or a specific tag (via `AKK_VERSION`, which
/// both install.sh and install.ps1 already understand) and overwrites the
/// current binary in place. Asks for interactive confirmation first, then
/// runs the installer quietly in the background behind a spinner. Kept
/// dependency-free by shelling out to curl/sh (or PowerShell on Windows)
/// instead of pulling in an HTTP client crate.
fn cmd_update(version: Option<&str>) {
    let label = version.unwrap_or("latest");

    if !ui::confirm(&format!("akk ကို ဗားရှင်း \"{}\" သို့ အပ်ဒိတ် လုပ်မလား။", label)) {
        println!("    ပယ်ဖျက်လိုက်ပါသည်။");
        return;
    }

    println!("==> akk ကို ဗားရှင်း \"{}\" သို့ အပ်ဒိတ် လုပ်နေသည်...", label);

    let version_owned = version.map(|s| s.to_string());
    let spinner_message = format!("akk {} ကို ဒေါင်းလုဒ် လုပ်နေသည်...", label);

    let outcome = ui::with_spinner(
        &spinner_message,
        move || -> std::io::Result<std::process::Output> {
            #[cfg(target_os = "windows")]
            {
                let url = format!(
                    "https://raw.githubusercontent.com/{}/main/scripts/install.ps1",
                    REPO
                );
                let version_arg = version_owned.as_deref().unwrap_or("latest");
                let ps_cmd = format!("$env:AKK_VERSION = '{}'; irm {} | iex", version_arg, url);
                Command::new("powershell")
                    .args([
                        "-NoProfile",
                        "-ExecutionPolicy",
                        "Bypass",
                        "-Command",
                        &ps_cmd,
                    ])
                    .output()
            }

            #[cfg(not(target_os = "windows"))]
            {
                let url = format!(
                    "https://raw.githubusercontent.com/{}/main/scripts/install.sh",
                    REPO
                );
                let shell_cmd = format!("curl -fsSL {} | sh", url);
                let mut cmd = Command::new("sh");
                cmd.arg("-c").arg(&shell_cmd);
                if let Some(v) = &version_owned {
                    cmd.env("AKK_VERSION", v);
                }
                cmd.output()
            }
        },
    );

    match outcome {
        Ok(output) if output.status.success() => {
            println!(
                "    [OK] ဗားရှင်း {} သို့ အပ်ဒိတ် ဖြစ်ပါပြီ။ စစ်ဆေးရန် 'akk --version' ကို ဖွင့်ပါ။",
                label
            );
        }
        Ok(output) => {
            eprintln!("    [FAILED] updater သည် status {} ဖြင့် ထွက်သွားပါသည်", output.status);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.trim().is_empty() {
                eprintln!("{}", stderr.trim_end());
            }
            process::exit(1);
        }
        Err(e) => {
            eprintln!("    [FAILED] installer ကို ဖွင့်၍မရပါ - {}", e);
            eprintln!(
                "    curl (သို့မဟုတ် PowerShell) ရရှိနိုင်ပြီး ကွန်ရက် ချိတ်ဆက်မှု ရှိကြောင်း စစ်ပါ။"
            );
            process::exit(1);
        }
    }
}

/// A download outcome handed back from the spinner's background thread, so
/// the error message / process::exit happens on the main thread only after
/// the spinner has finished cleaning up the terminal (cursor, line, etc.).
enum InstallDownload {
    Ok {
        files: Vec<(String, String)>,
        manifest: Option<String>,
    },
    NotFound,
    NetworkError(String),
    ReadError(String),
}

/// `akk install <name>` -- downloads a library from the Akkhara library
/// repo (`LIBRARY_REPO`) into the local `libraries/` folder next to the
/// akk binary, so `နည်းပညာများ <name> ကို သုံးမည်။` can find it later.
///
/// Libraries live at `<LIBRARY_PATH>/<name>/` in that repo: `main/`
/// holds the library's source file(s) (plain Akkhara, so no recompiling
/// akk is needed), and `index.json` is its manifest, carrying a
/// version/description shown after install.
fn cmd_install(name: &str) {
    if !ui::confirm(&format!("နည်းပညာ \"{}\" ကို install လုပ်မလား။", name)) {
        println!("    ပယ်ဖျက်လိုက်ပါသည်။");
        return;
    }

    println!("==> နည်းပညာ {} ကို install လုပ်နေသည်...", name);

    let dest_dir = libraries_dir().join(name);
    if let Err(e) = fs::create_dir_all(&dest_dir) {
        eprintln!(
            "    [FAILED] နည်းပညာ ဖိုဒါ {} ကို ဖန်တီး၍မရပါ - {}",
            dest_dir.display(),
            e
        );
        process::exit(1);
    }

    let lib_base = format!("{}/{}", library_raw_base(), name);
    let index_url = format!("{}/index.json", lib_base);
    // raw.githubusercontent.com has no directory listing, so the GitHub
    // contents API is used to enumerate every file under this library's
    // `main/` folder -- this is what lets a library ship more than a
    // single main.akk file.
    let contents_api_url = format!(
        "https://api.github.com/repos/{}/contents/{}/{}/main",
        LIBRARY_REPO, LIBRARY_PATH, name
    );
    let spinner_message = format!("Downloading \"{}\"...", name);

    let download = ui::with_spinner(&spinner_message, move || -> InstallDownload {
        // index.json is optional -- version/description are just nice to
        // show; installation still succeeds without it.
        let manifest = ureq::get(&index_url)
            .call()
            .ok()
            .and_then(|r| r.into_string().ok());

        let listing = match ureq::get(&contents_api_url)
            .set("User-Agent", "akk-cli")
            .call()
        {
            Ok(resp) => match resp.into_string() {
                Ok(s) => s,
                Err(e) => return InstallDownload::ReadError(e.to_string()),
            },
            Err(ureq::Error::Status(404, _)) => return InstallDownload::NotFound,
            Err(e) => return InstallDownload::NetworkError(e.to_string()),
        };

        let mut files = Vec::new();
        for obj in split_json_objects(&listing) {
            let Some(item_type) = extract_json_string_field(obj, "type") else {
                continue;
            };
            if item_type != "file" {
                continue; // subfolders inside main/ aren't supported yet
            }
            let (Some(file_name), Some(download_url)) = (
                extract_json_string_field(obj, "name"),
                extract_json_string_field(obj, "download_url"),
            ) else {
                continue;
            };
            match ureq::get(&download_url).call() {
                Ok(resp) => match resp.into_string() {
                    Ok(body) => files.push((file_name, body)),
                    Err(e) => return InstallDownload::ReadError(e.to_string()),
                },
                Err(e) => return InstallDownload::NetworkError(e.to_string()),
            }
        }

        if files.is_empty() {
            return InstallDownload::NotFound;
        }

        InstallDownload::Ok { files, manifest }
    });

    let (files, manifest_body) = match download {
        InstallDownload::Ok { files, manifest } => (files, manifest),
        InstallDownload::NotFound => {
            eprintln!(
                "    [FAILED] {} တွင် \"{}\" ဆိုသော နည်းပညာ မတွေ့ပါ",
                LIBRARY_REPO, name
            );
            suggest_libraries(name);
            process::exit(1);
        }
        InstallDownload::NetworkError(e) => {
            eprintln!("    [FAILED] နည်းပညာ repo သို့ ချိတ်ဆက်၍မရပါ - {}", e);
            eprintln!("    ကွန်ရက် ချိတ်ဆက်မှု ရှိကြောင်း စစ်ပါ။");
            process::exit(1);
        }
        InstallDownload::ReadError(e) => {
            eprintln!("    [FAILED] ဒေါင်းလုဒ် လုပ်ထားသော ရင်းမြစ်ကို ဖတ်၍မရပါ - {}", e);
            process::exit(1);
        }
    };

    let main_dir = dest_dir.join("main");
    if let Err(e) = fs::create_dir_all(&main_dir) {
        eprintln!(
            "    [FAILED] ဖိုဒါ {} ကို ဖန်တီး၍မရပါ - {}",
            main_dir.display(),
            e
        );
        process::exit(1);
    }
    for (file_name, body) in &files {
        let path = main_dir.join(file_name);
        if let Err(e) = fs::write(&path, body) {
            eprintln!("    [FAILED] ဖိုင် {} ကို ရေး၍မရပါ - {}", path.display(), e);
            process::exit(1);
        }
    }
    if let Some(manifest) = &manifest_body {
        let _ = fs::write(dest_dir.join("index.json"), manifest);
    }

    println!("    [OK] \"{}\" ကို {} တွင် တပ်ဆင်ပြီးပါပြီ", name, dest_dir.display());
    if let Some(manifest) = &manifest_body {
        if let Some(v) = extract_json_string_field(manifest, "version") {
            println!("    ဗားရှင်း - {}", v);
        }
        if let Some(d) = extract_json_string_field(manifest, "description") {
            println!("    {}", d);
        }
    }
    println!();
    println!("    ပရောဂျက်တွင် ဤသို့ သုံးပါ —");
    println!("        နည်းပညာများ {} ကို သုံးမည်။", name);
}

/// On an install miss, browses the repo's `index.json` (a flat list of
/// published package names) and prints anything related, so the person can
/// see what's actually available instead of guessing blind.
fn suggest_libraries(query: &str) {
    let index_url = format!("{}/index.json", library_raw_base());
    let Ok(resp) = ureq::get(&index_url).call() else {
        return;
    };
    let Ok(body) = resp.into_string() else {
        return;
    };
    let names = extract_json_string_array(&body, "packages");
    if names.is_empty() {
        return;
    }

    let query_lower = query.to_lowercase();
    let related: Vec<&String> = names
        .iter()
        .filter(|n| {
            let n_lower = n.to_lowercase();
            n_lower.contains(&query_lower) || query_lower.contains(&n_lower)
        })
        .collect();

    if !related.is_empty() {
        eprintln!("    ဤအမည် ဖြစ်ပါသလား —");
        for n in related {
            eprintln!("      - {}", n);
        }
    } else {
        eprintln!("    ရနိုင်သော နည်းပညာများ —");
        for n in names.iter().take(20) {
            eprintln!("      - {}", n);
        }
    }
}

/// `akk uninstall` -- removes the akk binary itself.
///
/// On Unix, a running binary can be unlinked while still executing, so this
/// deletes it directly. On Windows, the OS normally won't let a process
/// delete its own open executable, so a short-lived detached helper process
/// is spawned to delete it a moment after akk exits.
fn cmd_uninstall() {
    let exe_path = match env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("    [FAILED] akk ၏ executable ကို ရှာမတွေ့ပါ - {}", e);
            process::exit(1);
        }
    };

    println!("==> akk ကို ဖယ်ရှားနေသည်");
    println!("    ဖိုင် - {}", exe_path.display());

    #[cfg(target_os = "windows")]
    {
        let path_str = exe_path.display().to_string();
        let cmd = format!("ping 127.0.0.1 -n 2 >nul & del /f /q \"{}\"", path_str);
        match Command::new("cmd").args(["/C", &cmd]).spawn() {
            Ok(_) => println!("    [OK] ဤ လုပ်ငန်းစဉ် ပြီးဆုံးပါက akk ဖယ်ရှားခံရမည်။"),
            Err(e) => {
                eprintln!("    [FAILED] ဖယ်ရှားမှုကို အစီအစဉ် မဆွဲနိုင်ပါ - {}", e);
                eprintln!("    ကိုယ်တိုင် ဖျက်ပါ - {}", path_str);
                process::exit(1);
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        if let Err(e) = fs::remove_file(&exe_path) {
            eprintln!(
                "    [FAILED] {} ကို ဖျက်၍မရပါ - {}",
                exe_path.display(),
                e
            );
            process::exit(1);
        }
        println!("    [OK] {} ကို ဖယ်ရှားပြီးပါပြီ", exe_path.display());
    }

    println!();
    println!("    akk ၏ install ဖိုဒါကို PATH ထဲ ကိုယ်တိုင် ထည့်ထားပါက၊");
    println!("    shell profile (ဥပမာ ~/.bashrc, ~/.zshrc) မှ ထို လိုင်းကို ဖယ်ပါ။");
}

#[cfg(test)]
mod new_tests {
    use super::valid_project_name;

    #[test]
    fn accepts_plain_and_myanmar_names() {
        for name in ["demo", "my-app", "app_2", "မြန်မာ", "အက္ခရာ"] {
            assert!(valid_project_name(name), "{name} should be accepted");
        }
    }

    #[test]
    fn name_length_limit_is_255_bytes() {
        assert!(valid_project_name(&"a".repeat(255)));
        assert!(!valid_project_name(&"a".repeat(256)));
        // 85 Myanmar letters are 255 bytes; 86 are 258.
        assert!(valid_project_name(&"က".repeat(85)));
        assert!(!valid_project_name(&"က".repeat(86)));
    }

    #[test]
    fn rejects_unsafe_names() {
        for name in [
            "", ".", "..", "-x", "a/b", "a\\b", "my app", "a:b", "CON", "con.txt", "a*b", "a\tb",
        ] {
            assert!(!valid_project_name(name), "{name} should be rejected");
        }
    }
}
