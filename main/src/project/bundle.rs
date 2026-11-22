//! `akk build` -- one file that runs a program.
//!
//! A built executable is a **byte copy of this interpreter** with a small slot
//! patched inside it and a header, the program's source text and a trailer
//! appended after it. Nothing is translated: the copy contains the same
//! interpreter, and the program's source stays readable inside the file. A
//! built program starts no faster than `akk run` on the same source -- it only
//! saves typing the command.
//!
//! Layout of a built file:
//!
//! ```text
//! [ interpreter copy ][ header 40 ][ source ][ deps ][ trailer 24 ]
//!   ^ patched slot at some offset inside the copy
//! ```
//!
//! The slot is a `#[used]` static inside the image, so a plain `akk` carries it
//! too, unpatched (`format_version = 0`, `flags = 0`). Reading it is a volatile
//! load from our own memory: no file I/O, no allocation, nothing to detect for
//! a program that was not built.
//!
//! The slot's magic bytes are written **once**, as the static's initialiser.
//! `akk build` uses the running image's own slot bytes as the search needle, so
//! no second literal copy exists in `.rodata` and the count of matches stays
//! one.

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Size of the slot inside the image.
pub const SLOT_LEN: usize = 32;
/// Size of the bundle header.
const HEADER_LEN: usize = 40;
/// Size of the bundle trailer, at the very end of the file.
const TRAILER_LEN: usize = 24;

/// `format_version` written into the slot when `akk build` patches it.
const SLOT_FORMAT_VERSION: u32 = 1;
/// Slot `flags` bit 0: this image carries an appended program.
const SLOT_FLAG_BUNDLED: u32 = 1;

/// Bundle header/trailer version and kind this reader understands.
const BUNDLE_FORMAT: u16 = 1;
/// Payload is the program's source text (`[build] mode = "bundle"`).
const BUNDLE_KIND_PROGRAM: u16 = 0;
/// Payload is a compiled `bytecode::Program` (`[build] mode = "bytecode"`).
const BUNDLE_KIND_BYTECODE: u16 = 1;

/// Header magic at the start of the appended header.
const HEADER_MAGIC: [u8; 8] = *b"AKKBNDL1";
/// Trailer magic, the last eight bytes of the trailer.
const TRAILER_MAGIC: [u8; 8] = *b"AKKEND01";

/// A `[dependencies] { path = ... }` script, embedded under its library name.
pub const KIND_DEPENDENCY: u8 = 0;
/// A neighbouring `<name>.akk` script, embedded under its library name.
pub const KIND_SCRIPT: u8 = 1;

/// The slot. The first eight bytes are the only magic in the image; the rest is
/// zero until `akk build` patches it. Its length must stay [`SLOT_LEN`].
#[used]
static AKK_SLOT: [u8; SLOT_LEN] = *b"AKKSLOT1\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0";

/// One script carried inside a built file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedScript {
    pub kind: u8,
    pub name: String,
    pub source: String,
}

/// A validated built file, read back from memory.
pub struct Bundle {
    /// The program's source text. Empty for a bytecode bundle, which carries
    /// instructions instead.
    pub source: String,
    /// The compiled program, when this was built with
    /// `[build] mode = "bytecode"`. Read back with `bytecode::Program::decode`.
    pub bytecode: Option<Vec<u8>>,
    /// Embedded scripts, dependencies first (`KIND_DEPENDENCY`), then
    /// neighbouring scripts (`KIND_SCRIPT`), each in build order.
    pub deps: Vec<EmbeddedScript>,
    /// Header flags: bit0 has deps, bit1 transcript, bit2 warnings, bit3 timing.
    pub flags: u32,
    /// The executable this was read from; the `.akop` transcript is written
    /// beside it.
    pub anchor: PathBuf,
}

/// The embedded scripts of the running built file, set once at startup by
/// [`probe`]. Plain `akk` never sets it, so the loader's lookup is a single
/// `OnceLock::get` away from `None`.
static EMBEDDED: OnceLock<Vec<EmbeddedScript>> = OnceLock::new();

/// The slot as it is in the running image.
fn slot_bytes() -> [u8; SLOT_LEN] {
    // A volatile read keeps the optimiser from folding this to the initialiser:
    // the bytes in the file on disk are what matter, not what we started with.
    unsafe { std::ptr::read_volatile(std::ptr::addr_of!(AKK_SLOT)) }
}

/// Whether this image carries an appended program.
fn slot_is_bundled(slot: &[u8; SLOT_LEN]) -> bool {
    u32_at(slot, 12) & SLOT_FLAG_BUNDLED != 0
}

/// The slot's own magic, taken from the image rather than written out again.
#[cfg(test)]
fn slot_magic(slot: &[u8; SLOT_LEN]) -> [u8; 8] {
    let mut magic = [0u8; 8];
    magic.copy_from_slice(&slot[..8]);
    magic
}

/// Look for an embedded script by library name. Dependencies were written
/// first, so a name that is both a dependency and a neighbouring script
/// resolves to the dependency, as a plain run does.
///
/// The returned path is what the warning pass prints; the source is the text
/// the interpreter runs. A name written with a `.akk` suffix matches the same
/// script.
pub fn embedded_source(name: &str) -> Option<(PathBuf, String)> {
    let table = EMBEDDED.get()?;
    let wanted = name.strip_suffix(".akk").unwrap_or(name);
    table
        .iter()
        .find(|script| script.name.strip_suffix(".akk").unwrap_or(&script.name) == wanted)
        .map(|script| {
            (
                PathBuf::from(format!("{}.akk", wanted)),
                script.source.clone(),
            )
        })
}

/// Whether `name` is carried inside this built file. Used by the loader to
/// keep an embedded script ahead of an installed package of the same name.
pub fn has_embedded(name: &str) -> bool {
    embedded_source(name).is_some()
}

/// The first thing `main` does. Returns `None` for a plain `akk`, having done
/// nothing but a volatile load. For a built file it validates the appended
/// program, publishes the embedded table, and hands back the program to run.
pub fn probe() -> Option<Bundle> {
    let slot = slot_bytes();
    if !slot_is_bundled(&slot) {
        return None;
    }
    let anchor = match std::env::current_exe() {
        Ok(path) => path,
        Err(e) => {
            eprintln!("E159 ထုပ်ပိုးဖိုင်ကို ရှာမတွေ့ပါ - {}", e);
            std::process::exit(1);
        }
    };
    match read_bundle(&anchor, &slot) {
        Ok(bundle) => {
            let _ = EMBEDDED.set(bundle.deps.clone());
            Some(bundle)
        }
        Err(message) => {
            eprintln!("{}", message);
            std::process::exit(1);
        }
    }
}

// --- reading and validating -------------------------------------------------

/// A built file that passed every check.
#[derive(Debug)]
struct Parsed {
    source: String,
    bytecode: Option<Vec<u8>>,
    deps: Vec<EmbeddedScript>,
    flags: u32,
}

/// Read and validate the bundle at `path`, given the slot read from memory.
/// Every structural problem is `E159`; a version or kind we do not know is
/// `E160`. Nothing here executes program text.
pub fn read_bundle(path: &Path, slot: &[u8; SLOT_LEN]) -> Result<Bundle, String> {
    let prefix = u64_at(slot, 16);
    let mut file = File::open(path).map_err(|e| structural(&format!("ဖတ်၍မရပါ - {}", e)))?;
    let len = file
        .metadata()
        .map_err(|e| structural(&format!("အရွယ်အစား မသိပါ - {}", e)))?
        .len();

    // A built file is at least the copy plus a header and a trailer.
    if len < prefix.saturating_add((HEADER_LEN + TRAILER_LEN) as u64) {
        return Err(structural("ဖိုင်သည် ပျက်နေပါသည် (အရွယ်အစား မလုံလောက်ပါ)"));
    }

    let mut trailer = [0u8; TRAILER_LEN];
    file.seek(SeekFrom::Start(len - TRAILER_LEN as u64))
        .map_err(|e| structural(&format!("ဖတ်၍မရပါ - {}", e)))?;
    file.read_exact(&mut trailer)
        .map_err(|e| structural(&format!("ဖတ်၍မရပါ - {}", e)))?;
    if trailer[..8] != TRAILER_MAGIC {
        return Err(structural("နောက်ဆုံး အမြီးကို ရှာမတွေ့ပါ"));
    }

    let body_len = u64_at(&trailer, 8);
    let before_trailer = len - TRAILER_LEN as u64;
    if body_len < HEADER_LEN as u64 || body_len > before_trailer {
        return Err(structural("ထုပ်ပိုးဖိုင် အရွယ်အစား မမှန်ပါ"));
    }
    let header_off = before_trailer - body_len;
    if header_off != prefix {
        return Err(structural("အပေါက်နှင့် အမြီး မကိုက်ညီပါ"));
    }

    file.seek(SeekFrom::Start(header_off))
        .map_err(|e| structural(&format!("ဖတ်၍မရပါ - {}", e)))?;
    let mut body = vec![0u8; body_len as usize];
    file.read_exact(&mut body)
        .map_err(|e| structural(&format!("ဖတ်၍မရပါ - {}", e)))?;

    if crc32(&body) != u32_at(&trailer, 16) {
        return Err(structural("ထုပ်ပိုးဖိုင်၏ CRC မကိုက်ညီပါ"));
    }

    let parsed = parse_body(&body)?;
    Ok(Bundle {
        source: parsed.source,
        bytecode: parsed.bytecode,
        deps: parsed.deps,
        flags: parsed.flags,
        anchor: path.to_path_buf(),
    })
}

/// Validate a header+payload body (everything between the copy and the
/// trailer). Split out so a unit test can drive every defect directly.
fn parse_body(body: &[u8]) -> Result<Parsed, String> {
    if body.len() < HEADER_LEN {
        return Err(structural("ခေါင်းစီး မလုံလောက်ပါ"));
    }
    if body[..8] != HEADER_MAGIC {
        return Err(structural("ခေါင်းစီးကို ရှာမတွေ့ပါ"));
    }
    let format = u16_at(body, 8);
    let kind = u16_at(body, 10);
    if format != BUNDLE_FORMAT || (kind != BUNDLE_KIND_PROGRAM && kind != BUNDLE_KIND_BYTECODE) {
        return Err(format!(
            "E160 ထုပ်ပိုးဖိုင် ပုံစံ (ဗားရှင်း {}၊ အမျိုးအစား {}) ကို မသိပါ။",
            format, kind
        ));
    }

    let flags = u32_at(body, 12);
    let source_len = u64_at(body, 16);
    let deps_len = u64_at(body, 24);
    let source_crc = u32_at(body, 32);
    let deps_crc = u32_at(body, 36);

    let total = (HEADER_LEN as u64)
        .checked_add(source_len)
        .and_then(|n| n.checked_add(deps_len))
        .ok_or_else(|| structural("ထုပ်ပိုးဖိုင် အရွယ်အစား မမှန်ပါ"))?;
    if total != body.len() as u64 {
        return Err(structural("ခေါင်းစီး၏ အရွယ်အစားများ မကိုက်ညီပါ"));
    }

    let source_start = HEADER_LEN;
    let source_end = source_start + source_len as usize;
    let deps_end = source_end + deps_len as usize;

    if crc32(&body[source_start..source_end]) != source_crc {
        return Err(structural("program ၏ CRC မကိုက်ညီပါ"));
    }
    if crc32(&body[source_end..deps_end]) != deps_crc {
        return Err(structural("ထည့်သွင်းထားသော ဖိုင်၏ CRC မကိုက်ညီပါ"));
    }

    let payload = &body[source_start..source_end];
    let deps = decode_deps(&body[source_end..deps_end])?;
    // A bytecode payload is instructions, not text, and it never carries
    // scripts: the compiler refuses library calls before a build gets here.
    let (source, bytecode) = if kind == BUNDLE_KIND_BYTECODE {
        if !deps.is_empty() {
            return Err(structural("bytecode ထုပ်ပိုးဖိုင်တွင် နည်းပညာ မပါရပါ"));
        }
        (String::new(), Some(payload.to_vec()))
    } else {
        (
            String::from_utf8(payload.to_vec())
                .map_err(|_| structural("program သည် UTF-8 မဟုတ်ပါ"))?,
            None,
        )
    };

    Ok(Parsed {
        source,
        bytecode,
        deps,
        flags,
    })
}

/// `{ kind u8, name_len u16, name, src_len u64, source }`, repeated.
fn decode_deps(mut bytes: &[u8]) -> Result<Vec<EmbeddedScript>, String> {
    let mut deps = Vec::new();
    while !bytes.is_empty() {
        if bytes.len() < 3 {
            return Err(structural("ထည့်သွင်းထားသော ဖိုင်၏ ခေါင်းစီး မလုံလောက်ပါ"));
        }
        let kind = bytes[0];
        let name_len = u16_at(bytes, 1) as usize;
        if bytes.len() < 3 + name_len + 8 {
            return Err(structural("ထည့်သွင်းထားသော ဖိုင်၏ အမည် မလုံလောက်ပါ"));
        }
        let name = String::from_utf8(bytes[3..3 + name_len].to_vec())
            .map_err(|_| structural("ထည့်သွင်းထားသော ဖိုင်၏ အမည်သည် UTF-8 မဟုတ်ပါ"))?;
        let src_len = u64_at(bytes, 3 + name_len) as usize;
        let start = 3 + name_len + 8;
        if bytes.len() < start + src_len {
            return Err(structural("ထည့်သွင်းထားသော ဖိုင်၏ ရင်းမြစ် မလုံလောက်ပါ"));
        }
        let source = String::from_utf8(bytes[start..start + src_len].to_vec())
            .map_err(|_| structural("ထည့်သွင်းထားသော ဖိုင်၏ ရင်းမြစ်သည် UTF-8 မဟုတ်ပါ"))?;
        deps.push(EmbeddedScript { kind, name, source });
        bytes = &bytes[start + src_len..];
    }
    Ok(deps)
}

// --- writing ----------------------------------------------------------------

/// What a build embeds.
pub struct BuildInput<'a> {
    pub source: &'a str,
    /// Set for `[build] mode = "bytecode"`: the encoded instructions replace
    /// the source text as the payload.
    pub bytecode: Option<&'a [u8]>,
    pub deps: &'a [EmbeddedScript],
    pub run_flags: u32,
}

/// Copy the running image to `out`, patch its slot and append the program.
/// Returns the file's final length. On any failure the temporary file is
/// removed and `out` is left as it was.
pub fn write_bundle(out: &Path, input: &BuildInput<'_>) -> Result<u64, String> {
    let base =
        std::env::current_exe().map_err(|e| format!("E168 လက်ရှိ လုပ်ဆောင်နေသော ဖိုင်ကို ရှာမတွေ့ပါ - {}", e))?;
    if same_file(&base, out) {
        return Err("E161 လက်ရှိ run နေသော akk ဖိုင်ကို မရေးနိုင်ပါ။".to_string());
    }

    let slot = slot_bytes();
    if slot_is_bundled(&slot) {
        return Err("E161 ဤ akk သည် ထုပ်ပိုးဖိုင် ဖြစ်နေပြီး ထပ်မံ build ၍ မရပါ။".to_string());
    }

    let out_dir = out.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(out_dir).map_err(|e| format!("E168 ဖိုဒါ ဖန်တီး၍မရပါ - {}", e))?;

    let stem = out
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "bundle".to_string());
    let tmp = out_dir.join(format!(".{}.akk-tmp", stem));

    let result = write_into(&base, &tmp, &slot, input);
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
        return result;
    }

    if out.exists() {
        let _ = fs::remove_file(out);
    }
    fs::rename(&tmp, out).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("E168 ဖိုင်ကို နေရာချ၍မရပါ - {}", e)
    })?;
    Ok(result.unwrap_or(0))
}

fn write_into(
    base: &Path,
    tmp: &Path,
    slot: &[u8; SLOT_LEN],
    input: &BuildInput<'_>,
) -> Result<u64, String> {
    fs::copy(base, tmp).map_err(|e| format!("E168 akk ကို ကူး၍မရပါ - {}", e))?;

    let mut image = fs::read(tmp).map_err(|e| format!("E168 ဖိုင်ကို ဖတ်၍မရပါ - {}", e))?;
    let patched = patch_slot(&mut image, slot)?;
    fs::write(tmp, &image).map_err(|e| format!("E168 ဖိုင်ကို ရေး၍မရပါ - {}", e))?;

    let prefix_len = image.len() as u64;
    let body = encode_body(input);
    let trailer = encode_trailer(&body);

    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(tmp)
        .map_err(|e| format!("E168 ဖိုင်ကို ဖွင့်၍မရပါ - {}", e))?;
    file.write_all(&body)
        .and_then(|()| file.write_all(&trailer))
        .and_then(|()| file.sync_all())
        .map_err(|e| format!("E168 ဖိုင်ကို ရေး၍မရပါ - {}", e))?;
    drop(file);

    // Read the new file back with the same validator a run uses. Nothing is
    // executed: this only proves the bytes are what a run will accept.
    let checked = read_bundle(tmp, &patched)?;
    let payload_matches = match input.bytecode {
        Some(bytes) => checked.bytecode.as_ref().map(Vec::len) == Some(bytes.len()),
        None => checked.bytecode.is_none() && checked.source.len() == input.source.len(),
    };
    if !payload_matches {
        return Err("E161 ရေးပြီးသော ထုပ်ပိုးဖိုင်ကို ပြန်ဖတ်၍ မကိုက်ညီပါ။".to_string());
    }
    Ok(prefix_len + body.len() as u64 + TRAILER_LEN as u64)
}

/// Replace the single copy of the slot in `image`. `needle` is the 32 bytes
/// read from this process's own slot, so the search never needs a second
/// literal copy of the magic. Anything but exactly one match is `E161`.
fn patch_slot(image: &mut [u8], needle: &[u8; SLOT_LEN]) -> Result<[u8; SLOT_LEN], String> {
    let mut hits = Vec::new();
    if image.len() >= SLOT_LEN {
        for at in 0..=image.len() - SLOT_LEN {
            if &image[at..at + SLOT_LEN] == needle {
                hits.push(at);
            }
        }
    }
    if hits.len() != 1 {
        return Err(format!(
            "E161 အပေါက်ကို တစ်ခုတည်း ရှာမတွေ့ပါ ({} ခု တွေ့သည်)။",
            hits.len()
        ));
    }
    let at = hits[0];
    let mut patched = *needle;
    patched[8..12].copy_from_slice(&SLOT_FORMAT_VERSION.to_le_bytes());
    patched[12..16].copy_from_slice(&SLOT_FLAG_BUNDLED.to_le_bytes());
    patched[16..24].copy_from_slice(&(image.len() as u64).to_le_bytes());
    image[at..at + SLOT_LEN].copy_from_slice(&patched);
    Ok(patched)
}

fn encode_body(input: &BuildInput<'_>) -> Vec<u8> {
    let (payload, kind): (&[u8], u16) = match input.bytecode {
        Some(bytes) => (bytes, BUNDLE_KIND_BYTECODE),
        None => (input.source.as_bytes(), BUNDLE_KIND_PROGRAM),
    };
    let deps = encode_deps(input.deps);
    let mut body = Vec::with_capacity(HEADER_LEN + payload.len() + deps.len());
    body.extend_from_slice(&HEADER_MAGIC);
    body.extend_from_slice(&BUNDLE_FORMAT.to_le_bytes());
    body.extend_from_slice(&kind.to_le_bytes());
    body.extend_from_slice(&input.run_flags.to_le_bytes());
    body.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    body.extend_from_slice(&(deps.len() as u64).to_le_bytes());
    body.extend_from_slice(&crc32(payload).to_le_bytes());
    body.extend_from_slice(&crc32(&deps).to_le_bytes());
    body.extend_from_slice(payload);
    body.extend_from_slice(&deps);
    body
}

fn encode_deps(deps: &[EmbeddedScript]) -> Vec<u8> {
    let mut out = Vec::new();
    for dep in deps {
        let name = dep.name.as_bytes();
        let source = dep.source.as_bytes();
        out.push(dep.kind);
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(name);
        out.extend_from_slice(&(source.len() as u64).to_le_bytes());
        out.extend_from_slice(source);
    }
    out
}

fn encode_trailer(body: &[u8]) -> [u8; TRAILER_LEN] {
    let mut trailer = [0u8; TRAILER_LEN];
    trailer[..8].copy_from_slice(&TRAILER_MAGIC);
    trailer[8..16].copy_from_slice(&(body.len() as u64).to_le_bytes());
    trailer[16..20].copy_from_slice(&crc32(body).to_le_bytes());
    trailer
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn structural(detail: &str) -> String {
    format!("E159 ထုပ်ပိုးဖိုင်သည် ပျက်နေပါသည် - {}။", detail)
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn u64_at(bytes: &[u8], at: usize) -> u64 {
    let mut raw = [0u8; 8];
    raw.copy_from_slice(&bytes[at..at + 8]);
    u64::from_le_bytes(raw)
}

/// CRC-32/IEEE (reflected, polynomial `0xEDB88320`), written out here so the
/// build path needs no dependency.
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in bytes {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_deps() -> Vec<EmbeddedScript> {
        vec![
            EmbeddedScript {
                kind: KIND_DEPENDENCY,
                name: "Tools".to_string(),
                source: "fn add(a, b) {\n    return a + b;\n}\n".to_string(),
            },
            EmbeddedScript {
                kind: KIND_SCRIPT,
                name: "More".to_string(),
                source: "fn twice(x) {\n    return x * 2;\n}\n".to_string(),
            },
        ]
    }

    #[test]
    fn crc32_matches_the_reference_vector() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn header_and_payload_round_trip() {
        let deps = sample_deps();
        let input = BuildInput {
            source: "\"hi\" ကို ပြပါ။\n",
            bytecode: None,
            deps: &deps,
            run_flags: 0b1110,
        };
        let body = encode_body(&input);
        let parsed = parse_body(&body).expect("valid body");
        assert_eq!(parsed.source, input.source);
        assert_eq!(parsed.deps, deps);
        assert_eq!(parsed.flags, 0b1110);

        let trailer = encode_trailer(&body);
        assert_eq!(&trailer[..8], &TRAILER_MAGIC);
        assert_eq!(u64_at(&trailer, 8), body.len() as u64);
        assert_eq!(u32_at(&trailer, 16), crc32(&body));
    }

    #[test]
    fn unpatched_slot_is_not_bundled() {
        let slot = slot_bytes();
        // The magic is only ever written once, in the static's initialiser, so
        // this test checks its shape instead of repeating the bytes here.
        let magic = slot_magic(&slot);
        assert!(magic
            .iter()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()));
        assert_eq!(&magic[7..], &b"1"[..1]);
        assert_eq!(u32_at(&slot, 8), 0);
        assert!(!slot_is_bundled(&slot));
    }

    #[test]
    fn patching_sets_the_slot_and_is_unique() {
        // A stand-in image: the real slot bytes, surrounded by filler.
        let slot = slot_bytes();
        let mut image = vec![0xAAu8; 128];
        image.extend_from_slice(&slot);
        image.extend_from_slice(&[0xBBu8; 64]);
        let at = 128;
        let patched = patch_slot(&mut image, &slot).expect("one match");
        assert_eq!(&image[at..at + SLOT_LEN], &patched);
        assert!(slot_is_bundled(&patched));
        assert_eq!(u32_at(&patched, 8), SLOT_FORMAT_VERSION);
        // prefix_len is the whole image length at patch time, not the slot offset.
        assert_eq!(u64_at(&patched, 16), image.len() as u64);

        // Two copies is ambiguous, none is missing. Both cases start from an
        // unpatched image: patching one copy leaves exactly one pristine one.
        let mut two = vec![0xAAu8; 32];
        two.extend_from_slice(&slot);
        two.extend_from_slice(&slot);
        assert!(patch_slot(&mut two, &slot).unwrap_err().starts_with("E161"));
        let mut none = vec![0u8; 64];
        assert!(patch_slot(&mut none, &slot)
            .unwrap_err()
            .starts_with("E161"));
    }

    fn body_for(source: &str) -> Vec<u8> {
        let deps = sample_deps();
        let input = BuildInput {
            source,
            bytecode: None,
            deps: &deps,
            run_flags: 0,
        };
        encode_body(&input)
    }

    /// A bytecode payload travels as bytes: the body's kind says so, the
    /// payload comes back untouched, and no script may ride along.
    #[test]
    fn a_bytecode_body_round_trips() {
        let instructions = b"AKKBC001\x01\x00\x00\x00\x00\x00\x00\x00\x00";
        let input = BuildInput {
            source: "",
            bytecode: Some(instructions),
            deps: &[],
            run_flags: 0,
        };
        let body = encode_body(&input);
        let parsed = parse_body(&body).expect("valid body");
        assert_eq!(parsed.bytecode.as_deref(), Some(&instructions[..]));
        assert!(parsed.source.is_empty());
        assert!(parsed.deps.is_empty());

        // Deps with a bytecode payload are refused, not silently dropped.
        let deps = sample_deps();
        let mixed = BuildInput {
            source: "",
            bytecode: Some(instructions),
            deps: &deps,
            run_flags: 0,
        };
        let err = parse_body(&encode_body(&mixed)).unwrap_err();
        assert!(err.contains("နည်းပညာ"), "{err}");
    }

    #[test]
    fn rejects_a_bad_header_magic() {
        let mut body = body_for("x");
        body[0] = b'X';
        assert!(parse_body(&body).unwrap_err().starts_with("E159"));
    }

    #[test]
    fn rejects_an_unknown_version_or_kind() {
        let mut version = body_for("x");
        version[8..10].copy_from_slice(&9u16.to_le_bytes());
        assert!(parse_body(&version).unwrap_err().starts_with("E160"));

        let mut kind = body_for("x");
        kind[10..12].copy_from_slice(&7u16.to_le_bytes());
        assert!(parse_body(&kind).unwrap_err().starts_with("E160"));
    }

    #[test]
    fn rejects_lengths_that_do_not_add_up() {
        let mut body = body_for("x");
        let len = (u64_at(&body, 16)) + 1;
        body[16..24].copy_from_slice(&len.to_le_bytes());
        assert!(parse_body(&body).unwrap_err().starts_with("E159"));
    }

    #[test]
    fn rejects_a_changed_payload_byte() {
        let mut body = body_for("abcdef\n");
        let last = body.len() - 1;
        body[last] ^= 0xFF;
        assert!(parse_body(&body).unwrap_err().starts_with("E159"));
    }

    #[test]
    fn rejects_overflowing_lengths() {
        let mut body = body_for("x");
        body[16..24].copy_from_slice(&u64::MAX.to_le_bytes());
        body[24..32].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(parse_body(&body).unwrap_err().starts_with("E159"));
    }

    #[test]
    fn rejects_a_truncated_deps_blob() {
        let deps = sample_deps();
        let input = BuildInput {
            source: "x",
            bytecode: None,
            deps: &deps,
            run_flags: 0,
        };
        let mut body = encode_body(&input);
        body.pop();
        // The header still claims the old deps length, so the totals disagree.
        assert!(parse_body(&body).unwrap_err().starts_with("E159"));
    }
}
