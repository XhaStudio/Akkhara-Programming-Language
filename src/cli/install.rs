//! `akk install` / `akk uninstall`: library packages.

use super::*;
use std::env;
use std::fs;
use std::process::{self, Command};

/// `akk install` with no library name -- installs every `[dependencies]`
/// entry the project's manifest lists. `path` dependencies are already in the
/// project, so only version/git entries are downloaded; each one reuses
/// `cmd_install`.
pub(crate) fn cmd_install_manifest() {
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

/// A download outcome handed back from the spinner's background thread, so
/// the error message / process::exit happens on the main thread only after
/// the spinner has finished cleaning up the terminal (cursor, line, etc.).
pub(crate) enum InstallDownload {
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
pub(crate) fn cmd_install(name: &str) {
    if !ui::confirm(&format!("နည်းပညာ \"{}\" ကို install လုပ်မလား။", name))
    {
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
        for obj in crate::stdlib::json::split_objects(&listing) {
            let Some(item_type) = crate::stdlib::json::extract_string_field(obj, "type") else {
                continue;
            };
            if item_type != "file" {
                continue; // subfolders inside main/ aren't supported yet
            }
            let (Some(file_name), Some(download_url)) = (
                crate::stdlib::json::extract_string_field(obj, "name"),
                crate::stdlib::json::extract_string_field(obj, "download_url"),
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

    println!(
        "    [OK] \"{}\" ကို {} တွင် တပ်ဆင်ပြီးပါပြီ",
        name,
        dest_dir.display()
    );
    if let Some(manifest) = &manifest_body {
        if let Some(v) = crate::stdlib::json::extract_string_field(manifest, "version") {
            println!("    ဗားရှင်း - {}", v);
        }
        if let Some(d) = crate::stdlib::json::extract_string_field(manifest, "description") {
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
pub(crate) fn suggest_libraries(query: &str) {
    let index_url = format!("{}/index.json", library_raw_base());
    let Ok(resp) = ureq::get(&index_url).call() else {
        return;
    };
    let Ok(body) = resp.into_string() else {
        return;
    };
    let names = crate::stdlib::json::extract_string_array(&body, "packages");
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
pub(crate) fn cmd_uninstall() {
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
            eprintln!("    [FAILED] {} ကို ဖျက်၍မရပါ - {}", exe_path.display(), e);
            process::exit(1);
        }
        println!("    [OK] {} ကို ဖယ်ရှားပြီးပါပြီ", exe_path.display());
    }

    println!();
    println!("    akk ၏ install ဖိုဒါကို PATH ထဲ ကိုယ်တိုင် ထည့်ထားပါက၊");
    println!("    shell profile (ဥပမာ ~/.bashrc, ~/.zshrc) မှ ထို လိုင်းကို ဖယ်ပါ။");
}
