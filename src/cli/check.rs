//! `akk --check`: is a newer release published? (read-only)

use super::*;
use std::process::{self, Command};

/// `akk --check` -- looks up the latest GitHub release tag and compares it
/// against the version this binary was built with. Read-only: it never
/// downloads or installs anything, unlike `akk update`.
pub(crate) fn cmd_check() {
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
            eprintln!("    curl (သို့မဟုတ် PowerShell) ရရှိနိုင်ပြီး ကွန်ရက် ချိတ်ဆက်မှု ရှိကြောင်း စစ်ပါ။");
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
    let latest_tag = match crate::stdlib::json::extract_string_field(&body, "tag_name") {
        Some(t) => t,
        None => {
            eprintln!("    [FAILED] GitHub မှ နောက်ဆုံး release အချက်အလက်ကို ဖတ်၍မရပါ");
            if let Some(msg) = crate::stdlib::json::extract_string_field(&body, "message") {
                eprintln!("    GitHub ၏ အချက် - {}", msg);
            }
            process::exit(1);
        }
    };

    if latest_tag.is_empty() {
        eprintln!(
            "    [FAILED] GitHub မှ {} အတွက် release အချက်အလက် မပြန်လာပါ",
            REPO
        );
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
