//! The middle end: `hir` (the tree, with names resolved), `mir` (basic blocks
//! and explicit branches), and the lowering that produces them.
//!
//! What runs today: `akk ir <file>` parses a program, resolves its names and
//! prints both representations plus the symbol table, and `akk ir --opt`
//! prints the MIR again after `crate::opt` has folded constants and removed
//! what that made dead -- the two sections side by side are the comparison.
//! Nothing here executes a program: the interpreter is still what an Akkhara
//! program means.

pub(crate) mod builder;
pub(crate) mod dump;
pub(crate) mod hir;
pub(crate) mod lower;
pub(crate) mod mir;

/// How much of the pipeline a dump covers.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Options {
    /// Print the optimised MIR as a second section.
    pub(crate) optimize: bool,
}

/// The printed form of one program.
pub(crate) struct Dump {
    /// The symbol table, the HIR and (when it lowered) the MIR.
    pub(crate) text: String,
    /// The `E176` message when MIR refused a construct, so the caller can tell
    /// a complete dump from a partial one.
    pub(crate) refused: Option<String>,
}

/// Parse `source`, lower it, and return the HIR and MIR dump.
///
/// A program MIR cannot represent is not an error for the dump: the HIR is
/// printed, and the MIR section carries the `E176` message that says which
/// construct and line stopped it.
pub(crate) fn dump_source(source: &str, options: Options) -> Result<Dump, String> {
    use std::fmt::Write;

    let tokens = crate::frontend::lexer::lex(source)?;
    let stmts = crate::frontend::parser::parse(&tokens)?;
    let program = lower::hir(&stmts);
    let mut text = String::new();
    dump::hir(&mut text, &program);
    match lower::mir(&program) {
        Ok(mir) => {
            dump::mir(&mut text, &mir);
            if options.optimize {
                // The printed section above is already built, so the same
                // program is optimised in place from here.
                let mut optimised = mir;
                let report = crate::opt::optimize(&mut optimised);
                dump::mir_optimised(&mut text, &optimised, &report);
            }
            Ok(Dump {
                text,
                refused: None,
            })
        }
        Err(message) => {
            let _ = writeln!(text, "== mir ==");
            let _ = writeln!(text, "  {}", message);
            Ok(Dump {
                text,
                refused: Some(message),
            })
        }
    }
}

/// Same, for a file on disk.
pub(crate) fn dump_file(path: &str, options: Options) -> Result<Dump, String> {
    let source = std::fs::read_to_string(path)
        .map_err(|error| format!("ဖိုင် \"{}\" ကို ဖတ်၍မရပါ - {}", path, error))?;
    dump_source(&source, options)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(source: &str) -> String {
        dump_source(source, Options::default()).expect("lower").text
    }

    #[test]
    fn a_program_lowers_to_hir_and_mir() {
        let text = text("\"hi\" ကို ဖော်ပြပါ။");
        assert!(text.contains("== symbols =="), "{}", text);
        assert!(text.contains("print \"hi\""), "{}", text);
        assert!(text.contains("== mir =="), "{}", text);
        assert!(text.contains("bb0:"), "{}", text);
    }

    #[test]
    fn a_binding_is_one_symbol_in_the_dump() {
        let text = text("x သည် 1 ဖြစ်၏။\nx ကို ဖော်ပြပါ။");
        assert!(text.contains("#0 x var line 1"), "{}", text);
        assert!(text.contains("store #0 = 1"), "{}", text);
        assert!(text.contains("print #0"), "{}", text);
    }

    #[test]
    fn a_loop_becomes_basic_blocks_with_a_branch() {
        let source = "i သည် 0, 3 ထဲမှ တစ်ခုစီ 1 တိုးခြင်းဖြင့်\n    i ကို ဖော်ပြပါ။\nပြီး။";
        let text = text(source);
        assert!(text.contains("branch "), "{}", text);
        assert!(text.contains("#0 += 1"), "{}", text);
    }

    #[test]
    fn a_construct_outside_mir_is_reported_with_e176() {
        let dump = dump_source("1.2.3 ကို ဖော်ပြပါ။", Options::default()).expect("HIR still lowers");
        assert!(dump.text.contains("E176"), "{}", dump.text);
        assert!(dump.text.contains("1.2.3"), "{}", dump.text);
        assert!(dump.refused.is_some());
    }

    #[test]
    fn a_parse_error_is_the_parsers_own_message() {
        match dump_source("အကယ်၍ (", Options::default()) {
            Ok(_) => panic!("an unbalanced condition should not parse"),
            Err(error) => assert!(error.contains("E0"), "{}", error),
        }
    }

    #[test]
    fn the_opt_flag_prints_a_second_optimised_section() {
        let source =
            "အကယ်၍ (2 > 1) ဖြစ်လျှင်\n    \"yes\" ကို ဖော်ပြပါ။\nမဟုတ်လျှင်\n    \"no\" ကို ဖော်ပြပါ။\nပြီး။";
        let plain = dump_source(source, Options::default()).expect("lower");
        assert!(!plain.text.contains("optimised"), "{}", plain.text);

        let both = dump_source(source, Options { optimize: true }).expect("lower");
        assert!(both.text.contains("== mir =="), "{}", both.text);
        assert!(both.text.contains("== mir (optimised) =="), "{}", both.text);
        assert!(both.text.contains("folded 1 instructions"), "{}", both.text);
        // The unoptimised section still shows the branch it folded.
        let (before, after) = both
            .text
            .split_once("== mir (optimised) ==")
            .expect("two sections");
        assert!(before.contains("branch t"), "{}", before);
        assert!(!after.contains("branch t"), "{}", after);
    }
}
