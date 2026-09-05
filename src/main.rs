mod interpreter;
mod lexer;
mod library;
mod parser;
mod ui;

/// The Akkhara "အချိန်" (Time) library, compiled in from its own source
/// under `libraries/အချိန်/main.rs`. Programs load it with:
///     နည်းပညာများ အချိန် ကို အသုံးပြုပါ။
#[path = "../libraries/အချိန်/main.rs"]
mod time_library;

/// The Akkhara "ကျပန်း" (Random) library, compiled in from its own source
/// under `libraries/ကျပန်း/main.rs`. Programs load it with:
///     နည်းပညာများ ကျပန်း ကို အသုံးပြုပါ။
#[path = "../libraries/ကျပန်း/main.rs"]
mod random_library;

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::{self, Command};

/// Pulled from Cargo.toml at build time (`[package] version = "..."`).
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// GitHub repo the installer / updater pulls releases from.
const REPO: &str = "XhaStudio/Akkhara-Programming-Language";

/// GitHub repo that hosts installable Akkhara library packages. Each
/// package is a folder under `packages/<name>/` containing `main.akk`
/// (the library's source) and `metadata.json` (name/version/description).
/// `index.json` at the repo root lists every published package name, and
/// is what `akk install` browses when it can't find an exact match.
const LIBRARY_REPO: &str = "XhaStudio/Akkhara-Libraries/Libraries";

/// Where `akk install` places downloaded packages, and where
/// `နည်းပညာများ <name> ကို အသုံးပြုပါ။` looks for them at runtime: a
/// `libraries/` folder next to the akk binary itself (not the current
/// working directory), so it works no matter where a script is run from.
fn libraries_dir() -> PathBuf {
    env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("libraries")))
        .unwrap_or_else(|| PathBuf::from("libraries"))
}

fn print_usage() {
    eprintln!("Usage: akk <file name>");
    eprintln!("       akk --version | -v");
    eprintln!("       akk --check");
    eprintln!("       akk install <library>");
    eprintln!("       akk update [version]");
    eprintln!("       akk uninstall");
}

fn main() {
    // Make sure the libraries/ folder exists next to the binary. Older
    // installs (and fresh downloads of a release tarball/zip) don't ship
    // this folder, so create it lazily on every run rather than relying on
    // the install scripts alone.
    let _ = fs::create_dir_all(libraries_dir());

    let args: Vec<String> = env::args().collect();

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
        "install" => {
            if args.len() < 3 {
                eprintln!("အသုံးပြုပုံ: akk install <library>");
                process::exit(1);
            }
            cmd_install(&args[2]);
            return;
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

    let filename = &args[1];
    let src = match fs::read_to_string(filename) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("ဖိုင် \"{}\" ကို ဖတ်၍မရပါ - {}", filename, e);
            process::exit(1);
        }
    };

    let tokens = match lexer::lex(&src) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{}", e);
            process::exit(1);
        }
    };

    let stmts = match parser::parse(&tokens) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{}", e);
            process::exit(1);
        }
    };

    let mut interp = interpreter::Interpreter::new(libraries_dir());
    if let Err(e) = interp.run(&stmts) {
        eprintln!("{}", e);
        process::exit(1);
    }
}

/// `akk --check` -- looks up the latest GitHub release tag and compares it
/// against the version this binary was built with. Read-only: it never
/// downloads or installs anything, unlike `akk update`.
fn cmd_check() {
    println!("==> Checking for updates...");

    let api_url = format!("https://api.github.com/repos/{}/releases/latest", REPO);

    #[cfg(target_os = "windows")]
    let fetch_result = {
        let ps_cmd = format!(
            "(Invoke-RestMethod -Uri '{}' -Headers @{{ 'User-Agent' = 'akk-cli' }}).tag_name",
            api_url
        );
        Command::new("powershell")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps_cmd])
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
            eprintln!("    [FAILED] could not check for updates: {}", e);
            eprintln!("    Make sure curl (or PowerShell) is available and you have network access.");
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
            eprintln!("    [FAILED] could not read the latest release info from GitHub");
            if let Some(msg) = extract_json_string_field(&body, "message") {
                eprintln!("    GitHub says: {}", msg);
            }
            process::exit(1);
        }
    };

    if latest_tag.is_empty() {
        eprintln!("    [FAILED] GitHub returned no release info for {}", REPO);
        process::exit(1);
    }

    let latest_version = latest_tag.trim_start_matches('v');

    println!("    Installed: {}", VERSION);
    println!("    Latest:    {}", latest_version);
    println!();

    if latest_version == VERSION {
        println!("    [OK] You're on the latest version.");
    } else {
        println!("    [!] A different version is available.");
        println!("        Run 'akk update' to install it.");
    }
}

/// Pulls `"field": "value"` out of a flat JSON body without a JSON crate.
/// Good enough for GitHub's release API response shape and akk's own
/// metadata.json/index.json files; not a general parser.
#[allow(dead_code)]
fn extract_json_string_field(body: &str, field: &str) -> Option<String> {
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

    if !ui::confirm(&format!("Update akk to version \"{}\"?", label)) {
        println!("    Cancelled.");
        return;
    }

    println!("==> Updating akk to version \"{}\"...", label);

    let version_owned = version.map(|s| s.to_string());
    let spinner_message = format!("Downloading akk {}...", label);

    let outcome = ui::with_spinner(&spinner_message, move || -> std::io::Result<std::process::Output> {
        #[cfg(target_os = "windows")]
        {
            let url = format!(
                "https://raw.githubusercontent.com/{}/main/scripts/install.ps1",
                REPO
            );
            let version_arg = version_owned.as_deref().unwrap_or("latest");
            let ps_cmd = format!(
                "$env:AKK_VERSION = '{}'; irm {} | iex",
                version_arg, url
            );
            Command::new("powershell")
                .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps_cmd])
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
    });

    match outcome {
        Ok(output) if output.status.success() => {
            println!("    [OK] Updated to {}. Run 'akk --version' to confirm.", label);
        }
        Ok(output) => {
            eprintln!("    [FAILED] updater exited with status: {}", output.status);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.trim().is_empty() {
                eprintln!("{}", stderr.trim_end());
            }
            process::exit(1);
        }
        Err(e) => {
            eprintln!("    [FAILED] could not run the installer: {}", e);
            eprintln!("    Make sure curl (or PowerShell) is available and you have network access.");
            process::exit(1);
        }
    }
}

/// A download outcome handed back from the spinner's background thread, so
/// the error message / process::exit happens on the main thread only after
/// the spinner has finished cleaning up the terminal (cursor, line, etc.).
enum InstallDownload {
    Ok { source: String, metadata: Option<String> },
    NotFound,
    NetworkError(String),
    ReadError(String),
}

/// `akk install <name>` -- downloads a package from the Akkhara library
/// repo (`LIBRARY_REPO`) into the local `libraries/` folder next to the
/// akk binary, so `နည်းပညာများ <name> ကို အသုံးပြုပါ။` can find it later.
///
/// Packages live at `packages/<name>/` in that repo: `main.akk` is the
/// library's source (plain Akkhara, so no recompiling akk is needed), and
/// `metadata.json` carries a version/description shown after install.
fn cmd_install(name: &str) {
    if !ui::confirm(&format!("Install library \"{}\"?", name)) {
        println!("    Cancelled.");
        return;
    }

    println!("==> Installing library: {}...", name);

    let dest_dir = libraries_dir();
    if let Err(e) = fs::create_dir_all(&dest_dir) {
        eprintln!(
            "    [FAILED] could not create libraries folder {}: {}",
            dest_dir.display(),
            e
        );
        process::exit(1);
    }

    let base = format!(
        "https://raw.githubusercontent.com/{}/main/packages/{}",
        LIBRARY_REPO, name
    );
    let main_akk_url = format!("{}/main.akk", base);
    let metadata_url = format!("{}/metadata.json", base);
    let spinner_message = format!("Downloading \"{}\"...", name);

    let download = ui::with_spinner(&spinner_message, move || -> InstallDownload {
        let source = match ureq::get(&main_akk_url).call() {
            Ok(resp) => match resp.into_string() {
                Ok(s) => s,
                Err(e) => return InstallDownload::ReadError(e.to_string()),
            },
            Err(ureq::Error::Status(404, _)) => return InstallDownload::NotFound,
            Err(e) => return InstallDownload::NetworkError(e.to_string()),
        };

        // metadata.json is optional -- version/description are just nice to
        // show; installation still succeeds without it.
        let metadata = ureq::get(&metadata_url)
            .call()
            .ok()
            .and_then(|r| r.into_string().ok());

        InstallDownload::Ok { source, metadata }
    });

    let (source, metadata_body) = match download {
        InstallDownload::Ok { source, metadata } => (source, metadata),
        InstallDownload::NotFound => {
            eprintln!(
                "    [FAILED] no library named \"{}\" was found in {}",
                name, LIBRARY_REPO
            );
            suggest_libraries(name);
            process::exit(1);
        }
        InstallDownload::NetworkError(e) => {
            eprintln!("    [FAILED] could not reach the library repo: {}", e);
            eprintln!("    Make sure you have network access.");
            process::exit(1);
        }
        InstallDownload::ReadError(e) => {
            eprintln!("    [FAILED] could not read downloaded source: {}", e);
            process::exit(1);
        }
    };

    let pkg_dir = dest_dir.join(name);
    if let Err(e) = fs::create_dir_all(&pkg_dir) {
        eprintln!("    [FAILED] could not create {}: {}", pkg_dir.display(), e);
        process::exit(1);
    }
    let main_akk_path = pkg_dir.join("main.akk");
    if let Err(e) = fs::write(&main_akk_path, &source) {
        eprintln!("    [FAILED] could not write {}: {}", main_akk_path.display(), e);
        process::exit(1);
    }
    if let Some(meta) = &metadata_body {
        let _ = fs::write(pkg_dir.join("metadata.json"), meta);
    }

    println!("    [OK] Installed \"{}\" to {}", name, pkg_dir.display());
    if let Some(meta) = &metadata_body {
        if let Some(v) = extract_json_string_field(meta, "version") {
            println!("    Version: {}", v);
        }
        if let Some(d) = extract_json_string_field(meta, "description") {
            println!("    {}", d);
        }
    }
    println!();
    println!("    Use it in a program with:");
    println!("        နည်းပညာများ {} ကို အသုံးပြုပါ။", name);
}

/// On an install miss, browses the repo's `index.json` (a flat list of
/// published package names) and prints anything related, so the person can
/// see what's actually available instead of guessing blind.
fn suggest_libraries(query: &str) {
    let index_url = format!(
        "https://raw.githubusercontent.com/{}/main/index.json",
        LIBRARY_REPO
    );
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
        eprintln!("    Did you mean:");
        for n in related {
            eprintln!("      - {}", n);
        }
    } else {
        eprintln!("    Available libraries:");
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
            eprintln!("    [FAILED] could not locate akk's own binary: {}", e);
            process::exit(1);
        }
    };

    println!("==> Uninstalling akk");
    println!("    Binary: {}", exe_path.display());

    #[cfg(target_os = "windows")]
    {
        let path_str = exe_path.display().to_string();
        let cmd = format!("ping 127.0.0.1 -n 2 >nul & del /f /q \"{}\"", path_str);
        match Command::new("cmd").args(["/C", &cmd]).spawn() {
            Ok(_) => println!("    [OK] akk will be removed once this process exits."),
            Err(e) => {
                eprintln!("    [FAILED] could not schedule removal: {}", e);
                eprintln!("    Delete it manually: {}", path_str);
                process::exit(1);
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        if let Err(e) = fs::remove_file(&exe_path) {
            eprintln!("    [FAILED] could not remove {}: {}", exe_path.display(), e);
            process::exit(1);
        }
        println!("    [OK] Removed {}", exe_path.display());
    }

    println!();
    println!("    If you added akk's install directory to your PATH manually,");
    println!("    remove that line from your shell profile (e.g. ~/.bashrc, ~/.zshrc).");
}
