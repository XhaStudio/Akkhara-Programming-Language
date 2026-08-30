mod interpreter;
mod lexer;
mod parser;

use std::env;
use std::fs;
use std::process::{self, Command};

/// Pulled from Cargo.toml at build time (`[package] version = "..."`).
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// GitHub repo the installer / updater pulls releases from.
const REPO: &str = "XhaStudio/Akkhara-Programming-Language";

fn print_usage() {
    eprintln!("Usage: akk <file name>");
    eprintln!("       akk --version | -v");
    eprintln!("       akk --check");
    eprintln!("       akk update");
    eprintln!("       akk uninstall");
}

fn main() {
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
        "update" => {
            cmd_update();
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

    let mut interp = interpreter::Interpreter::new();
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
/// Good enough for GitHub's release API response shape; not a general parser.
#[cfg(not(target_os = "windows"))]
fn extract_json_string_field(body: &str, field: &str) -> Option<String> {
    let needle = format!("\"{}\":", field);
    let start = body.find(&needle)? + needle.len();
    let rest = body[start..].trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// `akk update` -- re-runs the official installer, which always fetches the
/// latest release and overwrites the current binary in place. Kept
/// dependency-free by shelling out to curl/sh (or PowerShell on Windows)
/// instead of pulling in an HTTP client crate.
fn cmd_update() {
    println!("==> Updating akk to the latest version...");

    #[cfg(target_os = "windows")]
    let result = {
        let url = format!(
            "https://raw.githubusercontent.com/{}/main/scripts/install.ps1",
            REPO
        );
        let ps_cmd = format!("irm {} | iex", url);
        Command::new("powershell")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps_cmd])
            .status()
    };

    #[cfg(not(target_os = "windows"))]
    let result = {
        let url = format!(
            "https://raw.githubusercontent.com/{}/main/scripts/install.sh",
            REPO
        );
        let shell_cmd = format!("curl -fsSL {} | sh", url);
        Command::new("sh").arg("-c").arg(&shell_cmd).status()
    };

    match result {
        Ok(status) if status.success() => {
            println!("    [OK] Update complete. Run 'akk --version' to confirm.");
        }
        Ok(status) => {
            eprintln!("    [FAILED] updater exited with status: {}", status);
            process::exit(1);
        }
        Err(e) => {
            eprintln!("    [FAILED] could not run the installer: {}", e);
            eprintln!("    Make sure curl (or PowerShell) is available and you have network access.");
            process::exit(1);
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
