//! The command line: `akk <file>`, `run`, `new`, `build`, `install`, `update`,
//! `--check`, `report`, `uninstall`.
//!
//! [`run`] is the whole entry point: it takes the program appended to a built
//! executable first, then reads the arguments and dispatches to one command
//! module. Nothing else belongs in this file.

pub(crate) mod build;
pub(crate) mod check;
pub(crate) mod install;
pub(crate) mod new;
pub(crate) mod progress;
pub(crate) mod run;
pub(crate) mod ui;
pub(crate) mod update;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

// The names the rest of the CLI used when this code lived in `main.rs`. The
// modules moved; the call sites read the same.
pub(crate) use crate::frontend::{lexer, parser};
pub(crate) use crate::interp as interpreter;
pub(crate) use crate::project::{bundle, library, manifest};
pub(crate) use crate::vm as bytecode;

/// Read the arguments and run one command. Returns the exit code the process
/// should end with.
///
/// The very first thing it does is check whether this executable *is* a
/// program: `akk build` writes a copy of this interpreter with one program
/// appended, and that file must run the program and nothing else -- before
/// the `libraries/` folder is created, before an argument is looked at, and
/// before any dispatch.
pub fn run() -> i32 {
    if let Some(bundle) = bundle::probe() {
        return run::run_bundle(bundle);
    }

    // Make sure the libraries/ folder exists next to the binary. Older
    // installs (and fresh downloads of a release tarball/zip) don't ship
    // this folder, so create it lazily on every run rather than relying on
    // the install scripts alone.
    let _ = fs::create_dir_all(libraries_dir());

    // `--no-warn` turns the static warnings off (see `sema::warnings`). It is
    // accepted before or after the file name / subcommand, so it is taken out
    // of the argument list here rather than parsed per command. `--vm` runs a
    // program on the bytecode compiler instead of the tree walker, and is
    // taken out the same way.
    //
    // Both flags are read from the untouched list on purpose. Commands used to
    // re-read `env::args()` *after* the filter below had removed `--no-warn`,
    // so `akk --vm --no-warn prog.akk` still printed warnings: the flag was
    // accepted but had no effect. The values computed here are the ones every
    // command receives now, which is why `run_bytecode` is given `no_warn`.
    let args: Vec<String> = env::args().collect();
    let no_warn = args.iter().any(|a| a == "--no-warn");
    let bytecode_mode = args.iter().any(|a| a == "--vm");
    // `--no-opt` is only read by `akk build <file>`: it compiles without the
    // MIR passes, so the slice can be checked with the optimiser off too.
    let no_opt = args.iter().any(|a| a == "--no-opt");
    let args: Vec<String> = args
        .into_iter()
        .filter(|a| a != "--no-warn" && a != "--vm" && a != "--no-opt")
        .collect();

    if args.len() < 2 {
        print_usage();
        return 1;
    }

    match args[1].as_str() {
        "--version" | "-v" | "version" => {
            println!("akk {}", VERSION);
            0
        }
        "--help" | "-h" | "help" => {
            print_usage();
            0
        }
        "--check" | "check" => {
            check::cmd_check();
            0
        }
        // `akk --vm <file>` -- compile to bytecode and run it.
        _ if bytecode_mode => run::run_bytecode(&args[1], no_warn),
        "report" => {
            let folder = args.get(2).map(String::as_str).unwrap_or("output");
            run::cmd_report(folder, no_warn);
            0
        }
        "install" => {
            // With no name, install everything the project's manifest lists;
            // with one, install exactly that library.
            if args.len() < 3 {
                install::cmd_install_manifest();
            } else {
                install::cmd_install(&args[2]);
            }
            0
        }
        "new" => new::cmd_new(args.get(2).map(String::as_str)),
        // `akk ir [--opt] <file>` -- print the pieces the middle end builds (the
        // symbol table, HIR and MIR) and run nothing. `--opt` adds the MIR
        // again after optimisation, so the two sections can be compared.
        "ir" => {
            let mut optimize = false;
            let mut file = None;
            for arg in &args[2..] {
                if arg.as_str() == "--opt" {
                    optimize = true;
                } else if file.is_none() {
                    file = Some(arg.as_str());
                }
            }
            match file {
                Some(file) => run::cmd_ir(file, optimize),
                None => {
                    eprintln!("Usage: akk ir [--opt] <file name>");
                    1
                }
            }
        }
        // `akk build <file>` writes a native executable beside the source;
        // with no file it bundles the project, as it always has.
        "build" => match args.get(2) {
            Some(file) => build::cmd_build_file(file, !no_opt),
            None => build::cmd_build(no_warn),
        },
        "run" => run::cmd_run(no_warn, args.get(2).map(String::as_str)),
        "update" => {
            let version = args.get(2).map(|s| s.as_str());
            update::cmd_update(version);
            0
        }
        "uninstall" => {
            install::cmd_uninstall();
            0
        }
        // A plain `akk file.akk` never looks for a manifest: its behavior is
        // exactly what it has always been. `akk run` is the manifest-aware
        // entry point (see `run::cmd_run`).
        _ => run::run_program(&args[1], run::RunOptions::defaults(no_warn), &[], &[], None),
    }
}

/// Pulled from Cargo.toml at build time (`[package] version = "..."`).
pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");

/// GitHub repo the installer / updater pulls releases from.
pub(crate) const REPO: &str = "XhaStudio/Akkhara-Programming-Language";

/// GitHub `owner/repo` that hosts installable Akkhara library packages.
pub(crate) const LIBRARY_REPO: &str = "XhaStudio/Akkhara-Libraries";

/// Path, within `LIBRARY_REPO`'s `main` branch, to the folder holding all
/// published libraries. Each library is a folder under
/// `<LIBRARY_PATH>/<name>/`: `main/` holds the library's source file(s)
/// (an entry `main.akk`, plus any others it needs), and `index.json` is
/// its manifest (name/version/description/entry). `<LIBRARY_PATH>/index.json`
/// lists every published library name, and is what `akk install` browses
/// when it can't find an exact match.
pub(crate) const LIBRARY_PATH: &str = "Libraries";

/// Base raw-content URL for `LIBRARY_PATH` on `LIBRARY_REPO`'s `main`
/// branch, e.g. `.../Libraries/<name>/index.json`.
pub(crate) fn library_raw_base() -> String {
    format!(
        "https://raw.githubusercontent.com/{}/main/{}",
        LIBRARY_REPO, LIBRARY_PATH
    )
}

/// Where `akk install` places downloaded packages, and where
/// `နည်းပညာများ <name> ကို သုံးမည်။` looks for them at runtime: a
/// `libraries/` folder next to the akk binary itself (not the current
/// working directory), so it works no matter where a script is run from.
pub(crate) fn libraries_dir() -> PathBuf {
    env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("libraries")))
        .unwrap_or_else(|| PathBuf::from("libraries"))
}

pub(crate) fn print_usage() {
    eprintln!("Usage: akk <file name> [--no-warn]");
    eprintln!("       akk run [file]             (akk.toml ပရောဂျက်ကို ဖွင့်သည်)");
    eprintln!("       akk new <name>             (ပရောဂျက် ဖိုဒါ အသစ် ဖန်တီးသည်)");
    eprintln!("       akk build                  (ပရောဂျက်ကို ဖွင့်ပေးမည့် executable တစ်ခု ရေးသည်)");
    eprintln!("       akk build <file name> [--no-opt]  (ဖိုင်ကို native executable အဖြစ် ရေးသည်)");
    eprintln!("       akk report [folder]        (ဖိုဒါထဲရှိ .akk အားလုံးကို ဖွင့်သည်)");
    eprintln!("       akk --version | -v");
    eprintln!("       akk --check");
    eprintln!("       akk --vm <file name>       (bytecode စက်ဖြင့် ဖွင့်သည်)");
    eprintln!("       akk ir [--opt] <file name>  (symbol table, HIR နှင့် MIR ကို ပြသည်၊ --opt ဖြင့် optimize ပြီး MIR ပါ ပြသည်)");
    eprintln!("       akk install [library]      (အမည် မပါလျှင် akk.toml မှီခိုချက်များကို install လုပ်သည်)");
    eprintln!("       akk update [version]");
    eprintln!("       akk uninstall");
}

/// A path as the user should see it: no Windows verbatim (`\\?\`) prefix,
/// which `canonicalize` and some shells add.
pub(crate) fn plain_path(path: &Path) -> String {
    let text = path.display().to_string();
    match text.strip_prefix(r"\\?\") {
        Some(rest) => rest.to_string(),
        None => text,
    }
}
