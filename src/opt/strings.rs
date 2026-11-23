//! String operations resolved at compile time.
//!
//! The native runtime carries no Unicode tables, so it cannot tell that
//! `မြန်မာ` is three characters rather than six scalar values. The compiler
//! can: `crate::text` is the interpreter's own cluster logic. For a string
//! whose contents are known before the program runs, this pass answers the
//! three operations that depend on cluster boundaries:
//!
//! * `len(s)` becomes an int constant;
//! * `for ch in s` becomes a list literal of the string's grapheme clusters
//!   (`[Str]`), so the loop walks it like any other list;
//! * `s[i]` becomes an index into a list literal of those clusters, so an
//!   out-of-range read still panics with the interpreter's `E049` and line.
//!
//! "Known" is the same single-definition constant analysis that constant
//! folding uses: a literal, or a binding that is assigned a literal exactly
//! once and is not a parameter. A string that is not known here -- a
//! parameter, a call result, a value read from a file -- is left alone, and
//! the analyzer refuses it by name with `E177` instead of emitting code that
//! would count bytes.
//!
//! The pass runs before the analyzer and regardless of `--no-opt`: this is
//! how the slice lowers string operations, not an optimisation. Running it
//! twice is a no-op, because the rewritten forms contain no string `Len`,
//! `Items`, `Index` or `len` call over known text.

use std::collections::HashSet;

use crate::ir::mir::*;
use crate::opt::const_fold::{constants, resolve, Known};

/// Rewrite every string operation whose text is known. Returns how many
/// instructions were replaced, for the caller's diagnostics.
pub(crate) fn run(program: &mut MirProgram) -> usize {
    // A user function named `len` wins over the builtin in the interpreter,
    // so a call to it must not be folded here either.
    let functions: HashSet<String> = program
        .functions
        .iter()
        .map(|function| function.name.clone())
        .collect();
    let mut rewrites = 0;
    for function in &mut program.functions {
        let known = constants(function);
        let mut next_temp = function.temps;
        for block in &mut function.blocks {
            let mut rewritten = Vec::with_capacity(block.insts.len());
            for instruction in block.insts.drain(..) {
                rewrites += rewrite(
                    instruction,
                    &known,
                    &functions,
                    &mut next_temp,
                    &mut rewritten,
                );
            }
            block.insts = rewritten;
        }
        function.temps = next_temp;
    }
    rewrites
}

/// The clusters of `text` as string-literal operands, in order.
fn cluster_items(text: &str) -> Vec<Operand> {
    crate::text::clusters(text)
        .into_iter()
        .map(|cluster| Operand::Const(Const::Str(cluster.to_string())))
        .collect()
}

/// Replace one instruction when it reads a string known at compile time;
/// otherwise write it through unchanged.
fn rewrite(
    instruction: MirInst,
    known: &Known,
    functions: &HashSet<String>,
    next_temp: &mut Temp,
    out: &mut Vec<MirInst>,
) -> usize {
    match &instruction {
        // `len(s)` is a builtin call, not a `Len` instruction.
        MirInst::Call { dest, name, args }
            if name == "len" && args.len() == 1 && !functions.contains(name) =>
        {
            if let Some(Const::Str(text)) = resolve(&args[0], known) {
                out.push(MirInst::Move {
                    dest: *dest,
                    value: Operand::Const(Const::Int(crate::text::len(&text))),
                });
                return 1;
            }
        }
        // The length a `for` loop reads after `Items` has already been
        // rewritten to a list, so this arm is a safety net for a `Len` that
        // reaches a string operand some other way.
        MirInst::Len { dest, value } => {
            if let Some(Const::Str(text)) = resolve(value, known) {
                out.push(MirInst::Move {
                    dest: *dest,
                    value: Operand::Const(Const::Int(crate::text::len(&text))),
                });
                return 1;
            }
        }
        MirInst::Items { dest, value } => {
            if let Some(Const::Str(text)) = resolve(value, known) {
                out.push(MirInst::List {
                    dest: *dest,
                    items: cluster_items(&text),
                });
                return 1;
            }
        }
        MirInst::Index {
            dest,
            base,
            indices,
            line,
        } if indices.len() == 1 => {
            if let Some(Const::Str(text)) = resolve(base, known) {
                // A fresh temporary holds the cluster list; the index then
                // reads it exactly as it would read any other list, which is
                // what keeps the bounds check and its `E049` text identical.
                let list = *next_temp;
                *next_temp += 1;
                out.push(MirInst::List {
                    dest: list,
                    items: cluster_items(&text),
                });
                out.push(MirInst::Index {
                    dest: *dest,
                    base: Operand::Temp(list),
                    indices: indices.clone(),
                    line: *line,
                });
                return 1;
            }
        }
        _ => {}
    }
    out.push(instruction);
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mir_for(source: &str) -> MirProgram {
        let tokens = crate::frontend::lexer::lex(source).expect("lex");
        let stmts = crate::frontend::parser::parse(&tokens).expect("parse");
        let hir = crate::ir::lower::hir(&stmts);
        crate::ir::lower::mir(&hir).expect("mir")
    }

    fn instructions(program: &MirProgram) -> Vec<&MirInst> {
        program.functions[0]
            .blocks
            .iter()
            .flat_map(|block| &block.insts)
            .collect()
    }

    /// `မြန်မာ` is three characters, so `len` of it folds to `3`, not `6`.
    #[test]
    fn a_known_string_length_becomes_a_constant() {
        let mut program = mir_for("x သည် len(\"မြန်မာ\") ဖြစ်၏။\n");
        assert_eq!(run(&mut program), 1);
        assert!(
            instructions(&program).iter().any(|instruction| matches!(
                instruction,
                MirInst::Move {
                    value: Operand::Const(Const::Int(3)),
                    ..
                }
            )),
            "{:?}",
            program
        );
    }

    /// A loop over known text walks the clusters as string literals, which is
    /// the `[Str]` list the analyzer types and the collection codegen emits.
    #[test]
    fn a_known_string_loop_becomes_a_list_of_clusters() {
        let mut program = mir_for("for ch in \"မြန်မာ\" {\n    print(ch);\n}\n");
        assert_eq!(run(&mut program), 1);
        let clusters: Vec<&Operand> = instructions(&program)
            .iter()
            .find_map(|instruction| match instruction {
                MirInst::List { items, .. } => Some(items.iter().collect()),
                _ => None,
            })
            .expect("a cluster list");
        assert_eq!(clusters.len(), 3);
        assert!(matches!(clusters[0], Operand::Const(Const::Str(text)) if text == "မြ"));
        assert!(matches!(clusters[2], Operand::Const(Const::Str(text)) if text == "မာ"));
    }

    /// A parameter has no compile-time text, so nothing is rewritten and the
    /// analyzer can refuse it by name.
    #[test]
    fn a_string_that_arrives_at_run_time_is_left_alone() {
        let mut program = mir_for(
            "fn show(s) {\n    for ch in s {\n        print(ch);\n    }\n}\nshow(\"abc\");\n",
        );
        assert_eq!(run(&mut program), 0);
    }

    /// Indexing and `len` of a binding assigned a literal once are known; a
    /// second assignment makes the binding dynamic again.
    #[test]
    fn a_single_constant_binding_is_known() {
        let mut program = mir_for("s သည် \"မြန်မာ\" ဖြစ်၏။\nprint(s[1]);\n");
        assert_eq!(run(&mut program), 1);

        let mut program = mir_for("s သည် \"မြန်မာ\" ဖြစ်၏။\nprint(s[1]);\ns သည် \"က\" ဖြစ်၏။\n");
        assert_eq!(run(&mut program), 0);
    }
}
