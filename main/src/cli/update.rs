//! `akk update [version]`: re-run the official installer.

use super::*;
use std::process::{self, Command};

/// `akk update [version]` -- re-runs the official installer, which fetches
/// either the latest release or a specific tag (via `AKK_VERSION`, which
/// both install.sh and install.ps1 already understand) and overwrites the
/// current binary in place. Asks for interactive confirmation first, then
/// runs the installer quietly in the background behind a spinner. Kept
/// dependency-free by shelling out to curl/sh (or PowerShell on Windows)
/// instead of pulling in an HTTP client crate.
pub(crate) fn cmd_update(version: Option<&str>) {
    let label = version.unwrap_or("latest");

    if !ui::confirm(&format!("akk ကို ဗားရှင်း \"{}\" သို့ အပ်ဒိတ် လုပ်မလား။", label))
    {
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
            eprintln!(
                "    [FAILED] updater သည် status {} ဖြင့် ထွက်သွားပါသည်",
                output.status
            );
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.trim().is_empty() {
                eprintln!("{}", stderr.trim_end());
            }
            process::exit(1);
        }
        Err(e) => {
            eprintln!("    [FAILED] installer ကို ဖွင့်၍မရပါ - {}", e);
            eprintln!("    curl (သို့မဟုတ် PowerShell) ရရှိနိုင်ပြီး ကွန်ရက် ချိတ်ဆက်မှု ရှိကြောင်း စစ်ပါ။");
            process::exit(1);
        }
    }
}
