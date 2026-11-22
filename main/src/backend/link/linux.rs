//! Writing a Linux executable: the bytes, then the executable bit.

use std::path::Path;

pub(crate) fn write_file(out: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(out, bytes)
        .map_err(|error| format!("E168 \"{}\" ကို ရေး၍မရပါ - {}", out.display(), error))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(out, std::fs::Permissions::from_mode(0o755)).map_err(|error| {
            format!(
                "E168 \"{}\" ကို run နိုင်အောင် ပြင်မ၍မရပါ - {}",
                out.display(),
                error
            )
        })?;
    }
    Ok(())
}
