//! A static `ET_EXEC` ELF64 image for Linux x86-64, written byte by byte.
//!
//! There is no linker on the build machine, so the file is the executable: one
//! program header maps the whole file read-write-execute at a fixed address,
//! which is all a program made of code, constants and syscalls needs.
//!
//! Layout: `[ELF header 64][program header 56][text][pad][data]`.

use crate::backend::x86_64::Image;

/// Where the image is mapped. Fixed, so absolute addresses baked into the code
/// stay valid without relocations.
pub(crate) const BASE: u64 = 0x40_0000;

const HEADER: usize = 64 + 56;

pub(crate) fn write(image: &mut Image) -> Vec<u8> {
    let text_start = HEADER;
    let mut data_start = text_start + image.text.len();
    while data_start % 8 != 0 {
        data_start += 1;
    }
    image.patch(BASE + data_start as u64);
    let entry = BASE + text_start as u64 + image.entry;
    let total = data_start + image.data.len();

    let mut file = Vec::with_capacity(total);
    // e_ident
    file.extend_from_slice(&[0x7F, b'E', b'L', b'F', 2, 1, 1, 0]);
    file.extend_from_slice(&[0u8; 8]);
    file.extend_from_slice(&2u16.to_le_bytes()); // e_type: ET_EXEC
    file.extend_from_slice(&0x3Eu16.to_le_bytes()); // e_machine: x86-64
    file.extend_from_slice(&1u32.to_le_bytes()); // e_version
    file.extend_from_slice(&entry.to_le_bytes());
    file.extend_from_slice(&64u64.to_le_bytes()); // e_phoff
    file.extend_from_slice(&0u64.to_le_bytes()); // e_shoff
    file.extend_from_slice(&0u32.to_le_bytes()); // e_flags
    file.extend_from_slice(&64u16.to_le_bytes()); // e_ehsize
    file.extend_from_slice(&56u16.to_le_bytes()); // e_phentsize
    file.extend_from_slice(&1u16.to_le_bytes()); // e_phnum
    file.extend_from_slice(&0u16.to_le_bytes()); // e_shentsize
    file.extend_from_slice(&0u16.to_le_bytes()); // e_shnum
    file.extend_from_slice(&0u16.to_le_bytes()); // e_shstrndx

    // The one program header: the whole file, mapped RWX.
    file.extend_from_slice(&1u32.to_le_bytes()); // p_type: PT_LOAD
    file.extend_from_slice(&7u32.to_le_bytes()); // p_flags: RWX
    file.extend_from_slice(&0u64.to_le_bytes()); // p_offset
    file.extend_from_slice(&BASE.to_le_bytes()); // p_vaddr
    file.extend_from_slice(&BASE.to_le_bytes()); // p_paddr
    file.extend_from_slice(&(total as u64).to_le_bytes()); // p_filesz
    file.extend_from_slice(&(total as u64).to_le_bytes()); // p_memsz
    file.extend_from_slice(&0x1000u64.to_le_bytes()); // p_align

    debug_assert_eq!(file.len(), text_start);
    file.extend_from_slice(&image.text);
    while file.len() < data_start {
        file.push(0);
    }
    file.extend_from_slice(&image.data);
    file
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
        compile(&program, Target::LinuxX86_64).expect("code")
    }

    #[test]
    fn the_header_is_a_static_x86_64_executable() {
        let mut image = image_for("\"hi\" ကို ဖော်ပြပါ။");
        let file = write(&mut image);
        assert_eq!(&file[0..4], b"\x7FELF");
        assert_eq!(file[4], 2, "64-bit");
        assert_eq!(u16::from_le_bytes([file[16], file[17]]), 2, "ET_EXEC");
        assert_eq!(u16::from_le_bytes([file[18], file[19]]), 0x3E, "x86-64");
    }

    #[test]
    fn the_entry_point_is_inside_the_text() {
        let mut image = image_for("1 ကို ဖော်ပြပါ။");
        let file = write(&mut image);
        let entry = u64::from_le_bytes(file[24..32].try_into().expect("entry"));
        assert_eq!(entry, BASE + HEADER as u64);
    }
}
