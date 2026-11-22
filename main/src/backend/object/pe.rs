//! A PE32+ console executable for Windows x86-64, written byte by byte.
//!
//! The Windows path needs no assembler or linker either: two sections hold the
//! code (read-execute) and the constant pool with the import table
//! (read-write). The image has a fixed base and no relocations, so the
//! absolute addresses the generator bakes in stay valid and the loader has
//! nothing to relocate.
//!
//! Layout: `[DOS stub][PE headers][.text][.idata]`.

use crate::backend::x86_64::Image;

/// Where the image is mapped; the code addresses are absolute.
pub(crate) const BASE: u64 = 0x1_4000_0000;
const HEADERS: usize = 0x200;
/// `.text` (the code) and `.idata` (constants, import slots, import table).
const TEXT_RVA: u32 = 0x1000;

/// Where `.idata` lands: the page after the code. The code has long outgrown
/// one page, so this is computed rather than fixed -- a fixed RVA would put the
/// section headers' virtual ranges on top of each other as soon as the runtime
/// grew, and the loader refuses such an image.
fn idata_rva(text_len: usize) -> u32 {
    TEXT_RVA + align(text_len, 0x1000) as u32
}

pub(crate) fn write(image: &mut Image) -> Result<Vec<u8>, String> {
    let data_rva = idata_rva(image.text.len());
    // The constants land at the start of `.idata`, so the data addresses the
    // code baked in are known before the file is built.
    image.patch(BASE + data_rva as u64);

    let mut idata = image.data.clone();
    let import = import_table(image, &mut idata, data_rva)?;

    let text_raw = align(image.text.len(), 0x200);
    let idata_raw = align(idata.len(), 0x200);
    let idata_offset = HEADERS + text_raw;
    let image_size = data_rva + align(idata.len(), 0x1000) as u32;
    let entry = TEXT_RVA + image.entry as u32;

    let mut file = vec![0u8; HEADERS];
    file[0] = b'M';
    file[1] = b'Z';
    put32(&mut file, 0x3C, 0x40); // e_lfanew
    file[0x40..0x44].copy_from_slice(b"PE\0\0");

    let coff = 0x44;
    put16(&mut file, coff, 0x8664); // machine: x86-64
    put16(&mut file, coff + 2, 2); // two sections
    put16(&mut file, coff + 16, 0xF0); // optional header size
    put16(&mut file, coff + 18, 0x0002 | 0x0001 | 0x0020); // executable, relocs stripped, large address aware

    let optional = coff + 20;
    put16(&mut file, optional, 0x20B); // PE32+
    put32(&mut file, optional + 4, text_raw as u32); // SizeOfCode
    put32(&mut file, optional + 8, idata_raw as u32); // SizeOfInitializedData
    put32(&mut file, optional + 16, entry); // AddressOfEntryPoint
    put32(&mut file, optional + 20, TEXT_RVA); // BaseOfCode
    put64(&mut file, optional + 24, BASE); // ImageBase
    put32(&mut file, optional + 32, 0x1000); // SectionAlignment
    put32(&mut file, optional + 36, 0x200); // FileAlignment
    put16(&mut file, optional + 40, 6); // MajorOperatingSystemVersion
    put16(&mut file, optional + 48, 6); // MajorSubsystemVersion
    put32(&mut file, optional + 56, image_size); // SizeOfImage
    put32(&mut file, optional + 60, HEADERS as u32); // SizeOfHeaders
    put16(&mut file, optional + 68, 3); // Subsystem: console
    put16(&mut file, optional + 70, 0x8100); // NX compatible, no dynamic base
    put64(&mut file, optional + 72, 0x10_0000); // SizeOfStackReserve
    put64(&mut file, optional + 80, 0x1000); // SizeOfStackCommit
    put64(&mut file, optional + 88, 0x10_0000); // SizeOfHeapReserve
    put64(&mut file, optional + 96, 0x1000); // SizeOfHeapCommit
    put32(&mut file, optional + 108, 16); // NumberOfRvaAndSizes
    let directories = optional + 112;
    put32(&mut file, directories + 8, import.0); // import directory RVA
    put32(&mut file, directories + 12, import.1); // and size

    let text_header = optional + 240;
    section(
        &mut file,
        text_header,
        ".text",
        image.text.len(),
        TEXT_RVA,
        text_raw,
        HEADERS,
        0x6000_0020, // code | execute | read
    );
    section(
        &mut file,
        text_header + 40,
        ".idata",
        idata.len(),
        data_rva,
        idata_raw,
        idata_offset,
        0xC000_0040, // initialized data | read | write
    );

    file.extend_from_slice(&image.text);
    file.resize(HEADERS + text_raw, 0);
    file.extend_from_slice(&idata);
    file.resize(idata_offset + idata_raw, 0);
    Ok(file)
}

/// One section header, 40 bytes.
fn section(
    file: &mut [u8],
    at: usize,
    name: &str,
    virtual_size: usize,
    virtual_address: u32,
    raw_size: usize,
    raw_offset: usize,
    characteristics: u32,
) {
    file[at..at + name.len()].copy_from_slice(name.as_bytes());
    put32(file, at + 8, virtual_size as u32);
    put32(file, at + 12, virtual_address);
    put32(file, at + 16, raw_size as u32);
    put32(file, at + 20, raw_offset as u32);
    put32(file, at + 36, characteristics);
}

/// Build one import descriptor for `kernel32.dll` over the slots the runtime
/// reserved, and return the directory's `(RVA, size)`.
///
/// `section` is the whole of `.idata`: the slots sit at the very start of the
/// constant pool, so `FirstThunk` is `base_rva` plus the first slot's offset.
fn import_table(image: &Image, section: &mut Vec<u8>, base_rva: u32) -> Result<(u32, u32), String> {
    if image.imports.is_empty() {
        return Ok((0, 0));
    }
    let first = image.imports[0].slot;
    for (index, import) in image.imports.iter().enumerate() {
        if import.slot != first + 8 * index as u32 {
            return Err("internal: the import slots are not one contiguous array".to_string());
        }
        if import.dll != "kernel32.dll" {
            return Err(format!(
                "internal: one import descriptor cannot hold \"{}\"",
                import.dll
            ));
        }
    }
    let iat = base_rva + first;

    align_to(section, 8);
    let ilt_offset = section.len();
    section.resize(ilt_offset + 8 * (image.imports.len() + 1), 0);
    // One descriptor plus an all-zero one that ends the table.
    let descriptor_offset = section.len();
    section.resize(descriptor_offset + 40, 0);

    let mut names = Vec::with_capacity(image.imports.len());
    for import in &image.imports {
        align_to(section, 2);
        let offset = section.len();
        section.extend_from_slice(&0u16.to_le_bytes()); // hint
        section.extend_from_slice(import.name.as_bytes());
        section.push(0);
        names.push(offset);
    }
    let dll_offset = section.len();
    section.extend_from_slice(b"kernel32.dll\0");

    // The IAT starts out holding the same hint/name RVAs as the lookup table,
    // the way a linker lays it out. This matters: the loader leaves an
    // all-zero slot unbound, so a zeroed IAT would leave every call jumping to
    // address zero.
    for (index, offset) in names.iter().enumerate() {
        let rva = (base_rva + *offset as u32) as u64;
        let at = ilt_offset + 8 * index;
        section[at..at + 8].copy_from_slice(&rva.to_le_bytes());
        let slot = first as usize + 8 * index;
        section[slot..slot + 8].copy_from_slice(&rva.to_le_bytes());
    }

    let descriptor = descriptor_offset;
    let original_first_thunk = base_rva + ilt_offset as u32;
    let name = base_rva + dll_offset as u32;
    section[descriptor..descriptor + 4].copy_from_slice(&original_first_thunk.to_le_bytes());
    section[descriptor + 12..descriptor + 16].copy_from_slice(&name.to_le_bytes());
    section[descriptor + 16..descriptor + 20].copy_from_slice(&iat.to_le_bytes());

    Ok((base_rva + descriptor as u32, 40))
}

fn align(value: usize, to: usize) -> usize {
    ((value + to - 1) / to) * to
}

fn align_to(bytes: &mut Vec<u8>, to: usize) {
    while bytes.len() % to != 0 {
        bytes.push(0);
    }
}

fn put16(file: &mut [u8], at: usize, value: u16) {
    file[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn put32(file: &mut [u8], at: usize, value: u32) {
    file[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn put64(file: &mut [u8], at: usize, value: u64) {
    file[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::x86_64::compile;
    use crate::runtime::Target;

    fn image_for(source: &str) -> Image {
        let tokens = crate::frontend::lexer::lex(source).expect("lex");
        let stmts = crate::frontend::parser::parse(&tokens).expect("parse");
        let hir = crate::ir::lower::hir(&stmts);
        let program = crate::ir::lower::mir(&hir).expect("mir");
        compile(&program, Target::WindowsX86_64).expect("code")
    }

    #[test]
    fn the_header_is_a_pe32_plus_console_image() {
        let mut image = image_for("\"hi\" ကို ဖော်ပြပါ။");
        let file = write(&mut image).expect("pe");
        assert_eq!(&file[0..2], b"MZ");
        assert_eq!(&file[0x40..0x44], b"PE\0\0");
        assert_eq!(u16::from_le_bytes([file[0x44], file[0x45]]), 0x8664);
        assert_eq!(u16::from_le_bytes([file[0x58], file[0x59]]), 0x20B);
        assert_eq!(u16::from_le_bytes([file[0x46], file[0x47]]), 2, "sections");
    }

    #[test]
    fn code_and_data_are_separate_sections() {
        let mut image = image_for("1 ကို ဖော်ပြပါ။");
        let file = write(&mut image).expect("pe");
        let text = 0x58 + 240;
        let idata = text + 40;
        assert_eq!(&file[text..text + 5], b".text");
        assert_eq!(&file[idata..idata + 6], b".idata");
        let text_flags = u32::from_le_bytes(file[text + 36..text + 40].try_into().unwrap());
        let idata_flags = u32::from_le_bytes(file[idata + 36..idata + 40].try_into().unwrap());
        assert_eq!(text_flags, 0x6000_0020);
        assert_eq!(idata_flags, 0xC000_0040);
    }

    #[test]
    fn the_import_directory_names_kernel32() {
        let mut image = image_for("1 ကို ဖော်ပြပါ။");
        let file = write(&mut image).expect("pe");
        assert!(file.windows(13).any(|window| window == b"kernel32.dll\0"));
        let directories = 0x58 + 112;
        let rva = u32::from_le_bytes(file[directories + 8..directories + 12].try_into().unwrap());
        let size = u32::from_le_bytes(file[directories + 12..directories + 16].try_into().unwrap());
        assert!(
            rva >= idata_rva(image.text.len()),
            "import directory RVA {:#x}",
            rva
        );
        assert_eq!(size, 40, "one descriptor plus its terminator");
    }
}
