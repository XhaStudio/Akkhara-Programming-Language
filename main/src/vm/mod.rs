//! The bytecode engine: instruction set, compiler and stack machine.
//!
//! `akk --vm <file>` and `[build] mode = "bytecode"` run programs through
//! this backend. The tree-walking interpreter in `interp` stays the
//! reference: every instruction produces the same values, the same output
//! and the same `E0xx` text.

mod compiler;
mod instr;
mod vm;

pub(crate) use compiler::*;
pub(crate) use instr::*;
pub(crate) use vm::*;

#[cfg(test)]
mod tests {
    use super::*;

    fn out(src: &str) -> Vec<String> {
        run_source(src).expect("the program runs on the VM")
    }

    #[test]
    fn if_chain_picks_one_branch() {
        let src = r#"x သည် 5 ဖြစ်၏။
(x > 3) ဖြစ်ရင်
    "big" ကို ဖော်ပြပါ။
အခြား (x > 10) မဖြစ်ရင်
    "huge" ကို ဖော်ပြပါ။
မဖြစ်ရင်
    "small" ကို ဖော်ပြပါ။
ပြီး။
"#;
        assert_eq!(out(src), vec!["big"]);
    }

    #[test]
    fn else_branch_runs_when_the_condition_is_false() {
        let src = r#"x သည် 1 ဖြစ်၏။
(x > 3) ဖြစ်ရင်
    "big" ကို ဖော်ပြပါ။
မဖြစ်ရင်
    "small" ကို ဖော်ပြပါ။
ပြီး။
"#;
        assert_eq!(out(src), vec!["small"]);
    }

    #[test]
    fn while_loop_counts_up() {
        let src = r#"i သည် 0 ဖြစ်၏။
(i < 3) ဖြစ်နေရင်
    i ကို ဖော်ပြပါ။
    i သည် i + 1 ဖြစ်၏။
ပြီး။
"#;
        assert_eq!(out(src), vec!["0", "1", "2"]);
    }

    #[test]
    fn for_range_and_collection_loops() {
        let src = r#"i သည် တစ်ခုစီ(0, 3)
    i ကို ဖော်ပြပါ။
ပြီး။
xs မှာ [7, 8, 9] ဖြစ်၏။
x သည် တစ်ခုစီ(xs)
    x ကို ဖော်ပြပါ။
ပြီး။
"#;
        assert_eq!(out(src), vec!["0", "1", "2", "7", "8", "9"]);
    }

    #[test]
    fn function_call_runs_its_body() {
        let src = r#"လုပ်ငန်း main()သည်
    "Hello, world" ကို ပြပါ။
ပြီး။
main ကို လုပ်ပါ။
"#;
        assert_eq!(out(src), vec!["Hello, world"]);
    }

    #[test]
    fn function_returns_its_last_expression() {
        let src = r#"လုပ်ငန်း double(n)သည်
    n * 2
ပြီး။
x သည် double(21) ဖြစ်၏။
x ကို ဖော်ပြပါ။
"#;
        assert_eq!(out(src), vec!["42"]);
    }

    #[test]
    fn arithmetic_and_string_concatenation() {
        let src = r#"a သည် 7 ဖြစ်၏။
a ကို 3 တိုးပါ။
a ကို ဖော်ပြပါ။
s မှာ "မြန်မာ" ဖြစ်၏။
s သည် s + " " + "စာ" ဖြစ်၏။
s ကို ဖော်ပြပါ။
"#;
        assert_eq!(out(src), vec!["10", "မြန်မာ စာ"]);
    }

    #[test]
    fn unsupported_statements_are_refused_with_e170() {
        // A library the VM cannot lower (`App` paints windows) is refused at
        // compile time; the value-only libraries compile.
        let err = run_source("use App;\nw = App.screen(400, 300);\n").unwrap_err();
        assert!(err.contains("E170"), "{err}");
        // A script/package library has no bytecode equivalent either.
        let err = run_source("နည်းပညာများ Tools ကို သုံးမည်။\n").unwrap_err();
        assert!(err.contains("E170"), "{err}");
    }

    /// Runs a program with scripted stdin and hands back the VM itself, so a
    /// test can look at the exact bytes the run wrote (`log`) as well as the
    /// lines it printed (`out`).
    fn run_with_input(src: &str, input: &str) -> Vm<'static> {
        let program: &'static Program = Box::leak(Box::new(
            compile_source(src).expect("the program compiles to bytecode"),
        ));
        let mut vm = Vm::new(program);
        vm.feed(input);
        vm.run().expect("the program runs on the VM");
        vm
    }

    #[test]
    fn input_single_read_keeps_the_prompt_and_the_value_on_one_line() {
        let src = "name :str = input(\"Name: \");\nname ကို ဖော်ပြပါ။\n";
        let vm = run_with_input(src, "David\n");
        assert_eq!(vm.out, vec!["David"]);
        // The prompt has no newline, so the answer lands on the same line.
        assert_eq!(vm.log, "Name: David\n");
    }

    #[test]
    fn input_inside_a_loop_reads_one_line_per_pass() {
        let src = r#"i သည် 0 ဖြစ်၏။
(i < 3) ဖြစ်နေရင်
    word :str = input("word: ");
    word ကို ဖော်ပြပါ။
    i သည် i + 1 ဖြစ်၏။
ပြီး။
"#;
        let vm = run_with_input(src, "a\nb\nc\n");
        assert_eq!(vm.out, vec!["a", "b", "c"]);
        assert_eq!(vm.log, "word: a\nword: b\nword: c\n");
    }

    #[test]
    fn input_feeding_a_numeric_comparison_is_converted() {
        let src = r#"n :int = input("n: ");
(n > 10) ဖြစ်ရင်
    "big" ကို ပြပါ။
မဖြစ်ရင်
    "small" ကို ပြပါ။
ပြီး။
"#;
        let vm = run_with_input(src, "42\n");
        assert_eq!(vm.out, vec!["big"]);
        assert_eq!(vm.log, "n: big\n");
    }

    #[test]
    fn input_is_consumed_by_a_function_call() {
        // The read binds a variable, and that value is handed to a function.
        let src = r#"လုပ်ငန်း shout(s)သည်
    s + "!"
ပြီး။
who :str = input("who? ");
msg သည် shout(who) ဖြစ်၏။
msg ကို ဖော်ပြပါ။
"#;
        let vm = run_with_input(src, "Zaw\n");
        assert_eq!(vm.out, vec!["Zaw!"]);
        assert_eq!(vm.log, "who? Zaw!\n");
    }
    #[test]
    fn prompts_are_written_as_the_program_reaches_them() {
        // Two reads with a print in between: the interleaving in `log` is the
        // program's own, not one the VM adds at the end.
        let src = "\"start\" ကို ပြပါ။\na :str = input(\"a: \");\na ကို ဖော်ပြပါ။\nb :str = input(\"b: \");\nb ကို ဖော်ပြပါ။\n";
        let vm = run_with_input(src, "one\ntwo\n");
        assert_eq!(vm.out, vec!["start", "one", "two"]);
        assert_eq!(vm.log, "start\na: one\nb: two\n");
    }

    #[test]
    fn a_typed_read_past_the_end_of_stdin_fails_loudly() {
        // An empty line cannot become a number, and the interpreter fails the
        // same way on the same line: the two must not diverge at end of input.
        let src = "n :int = input(\"n: \");\nn ကို ဖော်ပြပါ။\n";
        let err = run_source_with_input(src, "").unwrap_err();
        assert!(err.contains("E107"), "{err}");
        assert!(err.contains("လိုင်း 1"), "{err}");
    }

    // -----------------------------------------------------------------
    // eng spellings: declarations, assignments, print, calls, loop control
    // -----------------------------------------------------------------

    #[test]
    fn eng_declarations_and_assignments_run() {
        let src = "x :int = 5;\nx = x + 1;\nprint(x);\ns :str = \"a\";\ns += \"b\";\nprint(s);\ny :int = 10;\ny -= 4;\nprint(y);\nprint(true);\n";
        assert_eq!(out(src), vec!["6", "ab", "6", "True"]);
    }

    #[test]
    fn a_typed_declaration_refuses_the_wrong_value() {
        let err = run_source("x :int = \"no\";\n").unwrap_err();
        assert!(err.contains("E103"), "{err}");
        assert!(err.contains("လိုင်း 1"), "{err}");
    }

    #[test]
    fn a_pin_is_never_reassigned_and_never_redeclared() {
        let err = run_source("pin N :int = 1;\nN = 2;\n").unwrap_err();
        assert!(err.contains("E104"), "{err}");

        let err = run_source("pin N :int = 1;\npin N :int = 2;\n").unwrap_err();
        assert!(err.contains("E105"), "{err}");

        // A compound assignment is a write too.
        let err = run_source("pin N :int = 1;\nN += 1;\n").unwrap_err();
        assert!(err.contains("E104"), "{err}");
    }

    #[test]
    fn eng_print_and_a_discarded_call_run() {
        let src = "fn twice(n :int) {\n    return n * 2;\n}\ntwice(21);\nprint(twice(3));\n";
        assert_eq!(out(src), vec!["6"]);
    }

    #[test]
    fn interpolation_expands_at_the_literal_and_escapes_braces() {
        let src = "name = \"Zaw\";\nprint(\"Hi, {name}!\");\nprint(\"brace: \\{x\\}\");\nn = 3;\nprint(\"n={n}\");\n";
        assert_eq!(out(src), vec!["Hi, Zaw!", "brace: {x}", "n=3"]);
    }

    #[test]
    fn a_missing_interpolation_name_fails_like_the_interpreter() {
        let err = run_source("print(\"missing {zz}\");\n").unwrap_err();
        assert!(err.contains("E058"), "{err}");
        assert!(err.contains("zz"), "{err}");
    }

    #[test]
    fn eng_break_and_continue_inside_loops() {
        let src = "i သည် 0 ဖြစ်၏။\nwhile (i < 5) {\n    i = i + 1;\n    if (i == 2) {\n        continue;\n    }\n    if (i == 4) {\n        break;\n    }\n    print(i);\n}\nloop {\n    break;\n}\nprint(i);\n";
        assert_eq!(out(src), vec!["1", "3", "4"]);
    }

    #[test]
    fn loop_control_outside_a_loop_fails_with_the_interpreter_text() {
        let err = run_source("print(\"before\");\nbreak;\n").unwrap_err();
        assert!(err.contains("E116"), "{err}");

        let err = run_source("continue;\n").unwrap_err();
        assert!(err.contains("E134"), "{err}");
    }

    // -----------------------------------------------------------------
    // try / catch / finally
    // -----------------------------------------------------------------

    #[test]
    fn try_catches_the_error_by_code_and_runs_finally() {
        let src = "print(\"before\");\nစမ်းရန်\n    n :int = \"no\";\nE103 ကို ဖမ်းပါ။\n    print(\"caught\");\nနောက်ဆုံးတွင်\n    print(\"fin\");\nပြီး။\nprint(\"after\");\n";
        assert_eq!(out(src), vec!["before", "caught", "fin", "after"]);
    }

    #[test]
    fn a_try_without_an_error_still_runs_finally() {
        let src = "စမ်းရန်\n    print(\"try\");\nE103 ကို ဖမ်းပါ။\n    print(\"catch\");\nနောက်ဆုံးတွင်\n    print(\"fin\");\nပြီး။\nprint(\"after\");\n";
        assert_eq!(out(src), vec!["try", "fin", "after"]);
    }

    #[test]
    fn an_unmatched_error_runs_finally_and_then_escapes() {
        let src = "စမ်းရန်\n    n :int = \"no\";\nE999 ကို ဖမ်းပါ။\n    print(\"wrong\");\nနောက်ဆုံးတွင်\n    print(\"fin\");\nပြီး။\nprint(\"unreachable\");\n";
        let err = run_source(src).unwrap_err();
        assert!(err.contains("E103"), "{err}");
    }

    #[test]
    fn a_catch_for_any_error_catches_any_code() {
        let src = "စမ်းရန်\n    n :int = \"no\";\nE ကို ဖမ်းပါ။\n    print(\"any\");\nပြီး။\nprint(\"done\");\n";
        assert_eq!(out(src), vec!["any", "done"]);
    }

    #[test]
    fn an_error_from_a_called_function_is_caught_by_the_caller() {
        let src = "fn boom() {\n    x :int = \"no\";\n}\nစမ်းရန်\n    boom();\nE103 ကို ဖမ်းပါ။\n    print(\"caught a call\");\nပြီး။\nprint(\"done\");\n";
        assert_eq!(out(src), vec!["caught a call", "done"]);
    }

    #[test]
    fn an_error_inside_the_catch_body_runs_finally_and_escapes() {
        let src = "စမ်းရန်\n    n :int = \"no\";\nE103 ကို ဖမ်းပါ။\n    print(\"catch start\");\n    y :int = \"no\";\nနောက်ဆုံးတွင်\n    print(\"fin\");\nပြီး။\nprint(\"unreachable\");\n";
        let err = run_source(src).unwrap_err();
        assert!(err.contains("E103"), "{err}");
    }

    #[test]
    fn a_loop_inside_a_try_breaks_out_without_running_finally() {
        // The interpreter stops a statement list as soon as a break is in
        // flight, so `finally` is skipped on that exit -- and so is it here.
        let src = "i မှာ 0 ဖြစ်၏။\n(i < 5) ဖြစ်နေရင်\n    စမ်းရန်\n        i ကို 1 တိုးပါ။\n        (i == 2) ဖြစ်ရင်\n            ရပ်ပါ။\n        ပြီး။\n        \"body\" ကို ပြပါ။\n    နောက်ဆုံးတွင်\n        \"fin\" ကို ပြပါ။\n    ပြီး။\nပြီး။\n\"done\" ကို ပြပါ။\n";
        // The first pass ends normally (so the finally body runs); the second
        // one
        // breaks out of the loop, which skips it.
        assert_eq!(out(src), vec!["body", "fin", "done"]);
    }

    #[test]
    fn a_caught_error_inside_a_loop_is_caught_every_pass() {
        let src = "i မှာ 0 ဖြစ်၏။\n(i < 3) ဖြစ်နေရင်\n    စမ်းရန်\n        n :int = \"no\";\n    E103 ကို ဖမ်းပါ။\n        print(\"caught\");\n    ပြီး။\n    i ကို 1 တိုးပါ။\nပြီး။\nprint(\"done\");\n";
        assert_eq!(out(src), vec!["caught", "caught", "caught", "done"]);
    }

    #[test]
    fn a_nested_try_lets_the_outer_one_catch_an_unmatched_error() {
        let src = "စမ်းရန်\n    စမ်းရန်\n        n :int = \"no\";\n    E999 ကို ဖမ်းပါ။\n        print(\"inner wrong\");\n    ပြီး။\n    print(\"unreachable inner\");\nE103 ကို ဖမ်းပါ။\n    print(\"outer caught\");\nပြီး။\nprint(\"done\");\n";
        assert_eq!(out(src), vec!["outer caught", "done"]);
    }

    #[test]
    fn a_return_out_of_a_try_leaves_the_handler_behind() {
        // The return skips `finally` (the interpreter's signal rule), and the
        // error after the call must not be caught by the dead try.
        let src = "fn f(n :int) {\n    စမ်းရန်\n        if (n > 0) {\n            return \"early\";\n        }\n        return \"late\";\n    နောက်ဆုံးတွင်\n        print(\"fin\");\n    ပြီး။\n}\nprint(f(1));\nprint(f(0));\n";
        assert_eq!(out(src), vec!["early", "late"]);
    }

    /// An undefined callee is a *run-time* `E031` in the interpreter (after
    /// the lines before it have printed), so it must be one here too -- not a
    /// compile-time refusal.
    #[test]
    fn an_undefined_function_is_e031_at_run_time() {
        let src = "print(\"start\");\nnosuchfunction ကို လုပ်ပါ။\n";
        let err = run_source(src).unwrap_err();
        assert_eq!(err, "E031 လိုင်း 2 တွင် \"nosuchfunction\" ဆိုသော အလုပ်ကို ရှာမတွေ့ပါ။");
    }

    #[test]
    fn an_undefined_function_inside_a_try_is_catchable() {
        let src = "စမ်းရန်\n    nosuchthing ကို လုပ်ပါ။\nE031 ကို ဖမ်းပါ။\n    print(\"caught\");\nပြီး။\nprint(\"after\");\n";
        assert_eq!(out(src), vec!["caught", "after"]);
    }

    /// A wrong argument count is `E033` at run time, before the arguments run.
    #[test]
    fn a_wrong_argument_count_is_e033() {
        let src = "fn f(a :int) {\n    print(a);\n}\nprint(\"start\");\nf();\n";
        let err = run_source(src).unwrap_err();
        assert_eq!(
            err,
            "E033 လိုင်း 5 တွင် \"f\" အလုပ်သည် အချက် 1 ခု လိုအပ်ပါသည်၊ 0 ခု ပေးထားပါသည်။"
        );
        let src = "fn f(a :int) {\n    print(a);\n}\nf(1, 2);\n";
        let err = run_source(src).unwrap_err();
        assert_eq!(
            err,
            "E033 လိုင်း 4 တွင် \"f\" အလုပ်သည် အချက် 1 ခု လိုအပ်ပါသည်၊ 2 ခု ပေးထားပါသည်။"
        );
    }

    // -----------------------------------------------------------------
    // builtins
    // -----------------------------------------------------------------

    #[test]
    fn builtin_calls_compile_and_run() {
        let src = "print(len(\"abc\"));\nprint(abs(-5));\nprint(max(3, 7));\nprint(sqrt(6.25));\nprint(floor(2.9));\nprint(ceil(8.1));\nprint(round(1.4));\nprint(find(\"abcd\", \"d\"));\nprint(sqrt(16));\nprint(min(2.5, 3.5));\nprint(len([1, 2, 3, 4]));\nprint(sum([1, 1, 2]));\nprint(upper(\"eng\"));\nprint(trim(\"  Akkhara  \"));\nprint(contains(\"abc\", \"b\"));\nprint(starts_with(\"abc\", \"ab\"));\nprint(len(split(\"a,b,c,d\", \",\")));\n";
        assert_eq!(
            out(src),
            vec![
                "3", "5", "7", "2.5", "2", "9", "1", "3", "4.0", "2.5", "4", "4", "ENG", "Akkhara",
                "True", "True", "4"
            ]
        );
    }

    #[test]
    fn a_builtin_error_matches_the_interpreter() {
        // `sqrt(-1)` is `E111`, raised when the call runs -- and catchable.
        let src = "စမ်းရန်\n    sqrt(-1);\nE111 ကို ဖမ်းပါ။\n    print(\"sqrt ok\");\nပြီး။\nprint(\"done\");\n";
        assert_eq!(out(src), vec!["sqrt ok", "done"]);
        let err = run_source("print(\"before\");\nsqrt(-1);\n").unwrap_err();
        assert!(err.contains("E111"), "{err}");
    }

    #[test]
    fn a_builtin_is_checked_for_the_right_argument_type() {
        // The interpreter's own `E110` for `abs("x")`, not a VM-specific one.
        let err = run_source("print(abs(\"x\"));\n").unwrap_err();
        assert!(err.contains("E110"), "{err}");
    }

    #[test]
    fn reverse_compiles_as_a_statement_and_as_a_value() {
        // `reverse(xs);` discards the result, `print(reverse(xs))` keeps it;
        // both compile, and the list-mutating builtins still do too.
        assert!(compile_source("xs မှာ [1, 2] ဖြစ်၏။\nreverse(xs);\nprint(xs);\n").is_ok());
        assert!(compile_source("print(reverse([1, 2]));\n").is_ok());
        assert!(compile_source("xs မှာ [1] ဖြစ်၏။\nprint(push(xs, 2));\n").is_ok());
    }

    // -----------------------------------------------------------------
    // mutating builtins
    // -----------------------------------------------------------------

    #[test]
    fn the_mutating_builtins_change_their_list_and_return_the_useful_value() {
        let src = r#"xs မှာ [3, 1, 2] ဖြစ်၏။
print(push(xs, 4));
print(xs);
print(pop(xs));
print(sort(xs));
print(insert(xs, 0, 9));
print(remove(xs, 0));
print(append(xs, 7));
print(xs);
"#;
        assert_eq!(
            out(src),
            vec![
                "4",
                "[3, 1, 2, 4]",
                "4",
                "[1, 2, 3]",
                "4",
                "9",
                "4",
                "[1, 2, 3, 7]",
            ]
        );
        // As a statement and through the assigning call form.
        let src =
            "xs မှာ [2, 1] ဖြစ်၏။\nsort(xs);\nprint(xs);\ny အတွက် push(xs, 3) ကို လုပ်ပါ။\nprint(y);\n";
        assert_eq!(out(src), vec!["[1, 2]", "3"]);
    }

    #[test]
    fn a_mutating_builtin_keeps_the_interpreters_errors() {
        // A first argument that is not a variable: `E110`, catchable.
        let src = "စမ်းရန်\n    print(push(5, 1));\nE110 ကို ဖမ်းပါ။\n    print(\"caught\");\nပြီး။\n";
        assert_eq!(out(src), vec!["caught"]);
        // A missing variable: `E030`.
        let err = run_source("print(push(nosuch, 1));\n").unwrap_err();
        assert!(err.contains("E030"), "{err}");
        // Wrong argument count and an index out of range: `E110`/`E049`.
        let err = run_source("xs မှာ [1] ဖြစ်၏။\nprint(pop(xs, 2));\n").unwrap_err();
        assert!(err.contains("E110"), "{err}");
        let err = run_source("xs မှာ [1] ဖြစ်၏။\nprint(insert(xs, 9, 5));\n").unwrap_err();
        assert!(err.contains("E049"), "{err}");
        // Popping an empty list.
        let err = run_source("xs မှာ [] ဖြစ်၏။\nprint(pop(xs));\n").unwrap_err();
        assert!(err.contains("E110"), "{err}");
    }

    // -----------------------------------------------------------------
    // default parameters and the interpreter's dynamic scope
    // -----------------------------------------------------------------

    #[test]
    fn a_default_parameter_fills_in_before_the_body_runs() {
        let src = r#"fn greet(name, punct = "!") {
    return name + punct;
}
print(greet("A"));
print(greet("B", "?"));
fn f(a, b = a + 1, c = b * 2) {
    return [a, b, c];
}
print(f(1));
print(f(1, 5));
print(f(1, 5, 9));
"#;
        assert_eq!(
            out(src),
            vec!["A!", "B?", "[1, 2, 4]", "[1, 5, 10]", "[1, 5, 9]"]
        );
        // Too few arguments is still the interpreter's `E033`.
        let err = run_source("fn f(a, b = a + 1) {\n    return a;\n}\nf();\n").unwrap_err();
        assert!(err.contains("E033"), "{err}");
        // A lambda in a callback position may have one too.
        // An arrow lambda takes no defaults; the `fn` spelling does.
        let src = "print(map([1, 2], fn(x, k = 10) { x * k }));\n";
        assert_eq!(out(src), vec!["[10, 20]"]);
    }

    /// The interpreter's scope chain is dynamic: a callee sees its caller's
    /// frames, so `f()` may read a local of the function that called it.
    #[test]
    fn a_callee_reads_a_callers_local() {
        let src = "fn f() {\n    print(x);\n}\nfn g() {\n    x = 5;\n    f();\n}\ng();\nprint(\"after\");\n";
        assert_eq!(out(src), vec!["5", "after"]);
    }

    #[test]
    fn a_list_literal_may_hold_expressions() {
        let src = "a = 2;\nprint([a, a + 1, \"x\"]);\nprint((a, a));\n";
        assert_eq!(out(src), vec!["[2, 3, \"x\"]", "(2, 2)"]);
    }

    // -----------------------------------------------------------------
    // json and file libraries
    // -----------------------------------------------------------------

    #[test]
    fn the_json_library_runs_on_values() {
        let src = r#"use json;
text :str = json.stringify([1, 2, 3]);
print(text);
print(json.parse(text));
print(json.parse("1"));
print(json.get(json.parse("\{\"a\": 1\}"), "a", "none"));
print(json.get(json.parse("[1]"), "missing", "fallback"));
"#;
        assert_eq!(out(src), vec!["[1,2,3]", "[1, 2, 3]", "1", "1", "fallback"]);
        // A parse failure is the interpreter's `E132`, catchable by code.
        let src = "use json;\nစမ်းရန်\n    print(json.parse(\"\\{bad\"));\nE132 ကို ဖမ်းပါ။\n    print(\"caught\");\nပြီး။\n";
        assert_eq!(out(src), vec!["caught"]);
        // An unimported library still fails with `E089`.
        let err = run_source("print(json.parse(\"1\"));\n").unwrap_err();
        assert!(err.contains("E089"), "{err}");
    }

    #[test]
    fn the_file_library_runs_on_values() {
        let path = "akk_bytecode_file_test.txt";
        let src = format!(
            "use file;\nfile.write(\"{path}\", \"one\\ntwo\\n\");\nprint(file.exists(\"{path}\"));\nprint(file.lines(\"{path}\"));\nfile.append(\"{path}\", \"three\");\nprint(file.read(\"{path}\"));\nfile.delete(\"{path}\");\nprint(file.exists(\"{path}\"));\n"
        );
        assert_eq!(
            out(&src),
            vec!["True", "[\"one\", \"two\"]", "one\ntwo\nthree", "False"]
        );
        // A missing file is a catchable error, not empty text.
        let src = format!(
            "use file;\nစမ်းရန်\n    print(file.read(\"{path}\"));\nE140 ကို ဖမ်းပါ။\n    print(\"caught\");\nပြီး။\n"
        );
        let out_lines = out(&src);
        assert_eq!(out_lines.last().map(String::as_str), Some("caught"));
    }

    // -----------------------------------------------------------------
    // division and remainder
    // -----------------------------------------------------------------

    #[test]
    fn division_and_remainder_match_the_interpreter() {
        // Integer `/` yields a float and `%` is the Euclidean remainder, the
        // rules the interpreter's `numeric` follows.
        let src =
            "print(7 / 2);\nprint(-7 % 3);\nprint(7 % -3);\nprint(7 / 2.0);\nprint(-7.5 % 2.0);\n";
        assert_eq!(out(src), vec!["3.5", "2", "1", "3.5", "0.5"]);
    }

    /// Regression: a zero divisor reports the interpreter's code, text and
    /// line -- `E036`/`E038` for integers, `E037`/`E039` for floats -- from
    /// the line the operation was written on, wherever it runs.
    #[test]
    fn a_zero_divisor_reports_the_interpreters_code_and_line() {
        assert_eq!(
            run_source("print(1);\nprint(4 / 0);\n").unwrap_err(),
            "E036 လိုင်း 2 တွင် သုညဖြင့် စား၍မရပါ။"
        );
        assert_eq!(
            run_source("print(4 / 0.0);\n").unwrap_err(),
            "E037 လိုင်း 1 တွင် သုညဖြင့် စား၍မရပါ။"
        );
        assert_eq!(
            run_source("print(4 % 0);\n").unwrap_err(),
            "E038 လိုင်း 1 တွင် သုညဖြင့် ကြွင်းကိန်းရှာ၍မရပါ။"
        );
        assert_eq!(
            run_source("print(4 % 0.0);\n").unwrap_err(),
            "E039 လိုင်း 1 တွင် သုညဖြင့် ကြွင်းကိန်းရှာ၍မရပါ။"
        );
        // Inside a function the line is the one written there...
        assert_eq!(
            run_source("fn f(n :int) {\n    return n / 0;\n}\nf(1);\n").unwrap_err(),
            "E036 လိုင်း 2 တွင် သုညဖြင့် စား၍မရပါ။"
        );
        // ...and the error is a normal run-time one: catchable by its code.
        let src = "စမ်းရန်\n    print(1 % 0);\nE038 ကို ဖမ်းပါ။\n    print(\"caught\");\nပြီး။\n";
        assert_eq!(out(src), vec!["caught"]);
    }

    // -----------------------------------------------------------------
    // map / filter / reduce
    // -----------------------------------------------------------------

    #[test]
    fn the_callback_builtins_run_lambdas_and_named_functions() {
        let src = r#"xs မှာ [1, 2, 3] ဖြစ်၏။
print(map(xs, x => x * 2));
print(filter(xs, x => x > 1));
print(reduce(xs, (a, b) => a + b, 0));
fn double(n :int) {
    return n * 2;
}
print(map(xs, double));
print(reduce(xs, (a, b) => a * b, 1));
print(map("ab", c => c + "!"));
print(map([], x => x));
print(reduce([], (a, b) => a - b, 9));
print(map({3, 1, 2}, x => x + 1));
print(filter({3, 1, 2}, x => x > 1));
"#;
        assert_eq!(
            out(src),
            vec![
                "[2, 4, 6]",
                "[2, 3]",
                "6",
                "[2, 4, 6]",
                "6",
                "[\"a!\", \"b!\"]",
                "[]",
                "9",
                "[4, 2, 3]",
                "[3, 2]",
            ]
        );
    }

    #[test]
    fn a_lambda_sees_globals_and_may_call_another_callback_builtin() {
        let src = "xs မှာ [1, 2] ဖြစ်၏။\nk = 3;\nprint(map(xs, x => x * k));\nprint(map(xs, x => len(map(xs, y => y))));\n";
        assert_eq!(out(src), vec!["[3, 6]", "[2, 2]"]);
    }

    #[test]
    fn a_callback_builtin_keeps_the_interpreters_errors() {
        // A callback that returns something other than a bool: `E110`.
        let err = run_source("print(filter([1], x => x + 1));\n").unwrap_err();
        assert!(err.contains("E110"), "{err}");
        // A callback of the wrong arity: `E033`.
        let err = run_source("print(map([1], (a, b) => a));\n").unwrap_err();
        assert!(err.contains("E033"), "{err}");
        // A callback that returns nothing: `E114`.
        let src = "xs မှာ [1] ဖြစ်၏။\nfn shout(s :int) {\n    print(s);\n    return;\n}\nprint(map(xs, shout));\n";
        let err = run_source(src).unwrap_err();
        assert!(err.contains("E114"), "{err}");
        // A value that cannot be walked: `E110`.
        let err = run_source("print(map(5, x => x));\n").unwrap_err();
        assert!(err.contains("E110"), "{err}");
        // ...and `reduce`'s starting value is not evaluated before that check.
        let src = "fn noise() {\n    print(\"noise\");\n    return 0;\n}\nစမ်းရန်\n    print(reduce(5, (a, b) => a + b, noise()));\nE110 ကို ဖမ်းပါ။\n    print(\"caught\");\nပြီး။\nprint(\"done\");\n";
        assert_eq!(out(src), vec!["caught", "done"]);
    }

    #[test]
    fn an_error_inside_a_callback_is_caught_like_any_other() {
        // A handler outside the call takes over, and the loop does not resume.
        let src = "xs မှာ [1, 2] ဖြစ်၏။\nစမ်းရန်\n    print(map(xs, x => nosuchvar));\nE030 ကို ဖမ်းပါ။\n    print(\"caught\");\nပြီး။\nprint(\"after\");\n";
        assert_eq!(out(src), vec!["caught", "after"]);
        // A handler inside the lambda body keeps the walk going.
        let src = "xs မှာ [1, 2] ဖြစ်၏။\nprint(map(xs, fn(x) {\n    စမ်းရန်\n        return nosuchvar;\n    E030 ကို ဖမ်းပါ။\n        return -1;\n    ပြီး။\n}));\n";
        assert_eq!(out(src), vec!["[-1, -1]"]);
    }

    #[test]
    fn a_callback_builtin_compiles_as_a_statement_and_in_an_assignment() {
        let src =
            "xs မှာ [1, 2] ဖြစ်၏။\nmap(xs, x => x);\ny အတွက် map(xs, x => x + 1) ကို လုပ်ပါ။\nprint(y);\n";
        assert_eq!(out(src), vec!["[2, 3]"]);
    }

    #[test]
    fn a_callback_that_is_not_callable_is_e111() {
        // A plain value in the callback position now reaches the run, where
        // the interpreter's own E111 refuses it.
        let err = run_source("print(map([1], 3));\n").unwrap_err();
        assert!(err.contains("E110"), "{err}");
        assert!(err.contains("ပြန်ခေါ် အလုပ်"), "{err}");
    }

    #[test]
    fn a_library_callback_the_vm_cannot_represent_is_e170() {
        let err = compile_source("print(map([1], Tools.add));\n").unwrap_err();
        assert!(err.contains("E170"), "{err}");
    }

    // -----------------------------------------------------------------
    // a function's result, stored in a variable
    // -----------------------------------------------------------------

    /// Regression: `<var> အတွက် <fn>(...) ကို လုပ်ပါ။` compiles and the value is
    /// usable afterwards, exactly as under the interpreter.
    #[test]
    fn a_functions_result_is_stored_and_then_used() {
        let src = "လုပ်ငန်း twice အတွက် x ဖြင့်\n    x * 2\nပြီး။\ny အတွက် twice ကို လုပ်ရန် 21 ဖြင့်။\ny ကို ဖော်ပြပါ။\nprint(y + 1);\n";
        assert_eq!(out(src), vec!["42", "43"]);
    }

    #[test]
    fn a_body_that_ends_in_a_declaration_returns_that_value() {
        let src = "လုပ်ငန်း f အတွက် n ဖြင့်\n    r သည် n + 1 ဖြစ်၏။\nပြီးပါပြီ။\nx အတွက် f ကို လုပ်ရန် 41 ဖြင့်။\nx ကို ဖော်ပြပါ။\n".replace("ပြီးပါပြီ။", "ပြီး။");
        assert_eq!(out(&src), vec!["42"]);
    }

    #[test]
    fn a_call_that_returns_nothing_is_e071_when_stored() {
        // A body whose last statement is a print has no value.
        let src = "လုပ်ငန်း shout အတွက် s ဖြင့်\n    \"noise\" ကို ဖော်ပြပါ။\nပြီး။\nx အတွက် shout ကို လုပ်ရန် \"a\" ဖြင့်။\n";
        let err = run_source(src).unwrap_err();
        assert!(err.contains("E071"), "{err}");
        // A bare `return;` is the same.
        let err = run_source("fn v() {\n    print(\"side\");\n    return;\n}\ny အတွက် v ကို လုပ်ပါ။\n")
            .unwrap_err();
        assert!(err.contains("E071"), "{err}");
        // And it is catchable, as any run-time error is.
        let src = "fn v() {\n    print(\"side\");\n}\nစမ်းရန်\n    y အတွက် v ကို လုပ်ပါ။\nE071 ကို ဖမ်းပါ။\n    print(\"E071 ok\");\nပြီး။\n";
        assert_eq!(out(src), vec!["side", "E071 ok"]);
    }

    #[test]
    fn assigning_a_constructor_or_builtin_result_works() {
        let src = "ပုံသေ Point(x, y) သည်\n    လုပ်ငန်း Point()သည်\n        အတု px သည် x ဖြစ်၏။\n    ပြီး။\nပြီး။\np အတွက် Point(3, 4) ကို လုပ်ပါ။\np ၏ px ကို ဖော်ပြပါ။\nn အတွက် len(\"abcd\") ကို လုပ်ပါ။\nprint(n + 1);\n";
        assert_eq!(out(src), vec!["3", "5"]);
    }

    #[test]
    fn a_bad_callee_in_an_assigning_call_is_checked_first() {
        // Unknown function: E031, before the arguments would run.
        let err = run_source("z အတွက် nosuchfn(1) ကို လုပ်ပါ။\n").unwrap_err();
        assert!(err.contains("E031"), "{err}");
        // Wrong argument count: E033, before the arguments would run.
        let err = run_source("fn f(a :int) {\n    return a;\n}\nz အတွက် f() ကို လုပ်ပါ။\n").unwrap_err();
        assert!(err.contains("E033"), "{err}");
    }

    /// A missing variable reports `E030` with its line, as the interpreter
    /// does -- the value a stored call result leaves behind is no exception.
    #[test]
    fn a_missing_variable_reports_its_line() {
        let err = run_source("print(\"a\");\nprint(nosuchvar);\n").unwrap_err();
        assert_eq!(err, "E030 လိုင်း 2 တွင် \"nosuchvar\" ဆိုသော ကိန်းရှင်ကို ရှာမတွေ့ပါ။");
    }

    // -----------------------------------------------------------------
    // libraries
    // -----------------------------------------------------------------

    #[test]
    fn library_calls_compile_and_run() {
        let src = "use ကျပန်း;\nprint(ကျပန်း.ကိန်း(5, 6));\nprint(ကျပန်း.တန်ဖိုး([\"A\"]));\nprint(ကျပန်း.random_int(1, 2));\n";
        assert_eq!(out(src), vec!["5", "A", "1"]);
    }

    #[test]
    fn a_library_must_be_imported_before_it_is_called() {
        let err = run_source("print(ကျပန်း.ကိန်း(1, 2));\n").unwrap_err();
        assert!(err.contains("E089"), "{err}");
        assert_eq!(out("use ကျပန်း;\nprint(ကျပန်း.ကိန်း(1, 2));\n"), vec!["1"]);
    }

    #[test]
    fn library_errors_match_the_interpreter() {
        for (src, code) in [
            // unknown library
            ("nosuchlib.get(\"x\");\n", "E085"),
            // unknown function in a known library
            ("use ကျပန်း;\nprint(ကျပန်း.nosuchfn(1));\n", "E086"),
            // wrong argument count
            ("use ကျပန်း;\nprint(ကျပန်း.ကိန်း(1));\n", "E087"),
            // a call that acts without a value, used as one
            ("use အချိန်;\nprint(အချိန်.စောင့်(0));\n", "E117"),
            // a call that acts without a value, stored into a name
            ("use အချိန်;\nx အတွက် အချိန် ၏ စောင့်(0) ကို လုပ်ပါ။\n", "E088"),
        ] {
            let err = run_source(src).unwrap_err();
            assert!(err.contains(code), "{src} => {err}");
        }
    }

    #[test]
    fn a_library_alias_spelling_resolves_to_the_library() {
        let src = "နည်းပညာများ ကျပန်း အဖြစ် r ကို အသုံးပြုပါ။\nprint(r.ကိန်း(5, 6));\n";
        assert_eq!(out(src), vec!["5"]);
    }

    #[test]
    fn a_library_error_is_catchable_by_code() {
        let src = "စမ်းရန်\n    ကျပန်း.ကိန်း(1, 2);\nE089 ကို ဖမ်းပါ။\n    print(\"E089 ok\");\nပြီး။\nprint(\"done\");\n";
        assert_eq!(out(src), vec!["E089 ok", "done"]);
    }

    // -----------------------------------------------------------------
    // loop-as-value
    // -----------------------------------------------------------------

    #[test]
    fn a_loop_value_is_the_value_passed_to_break() {
        let src = "x :int = loop {\n    x += 1;\n    if (x == 3) {\n        break x * 10;\n    }\n};\nprint(x);\ny :str = loop {\n    break \"text\";\n};\nprint(y);\n";
        assert_eq!(out(src), vec!["30", "text"]);
    }

    #[test]
    fn a_break_without_a_value_in_a_value_loop_is_e115() {
        let src = "print(\"start\");\nv :int = loop {\n    break;\n};\nprint(v);\n";
        let err = run_source(src).unwrap_err();
        assert!(err.contains("E115"), "{err}");
        assert!(err.contains("လိုင်း 2"), "{err}");
    }

    #[test]
    fn a_nested_loop_value_targets_the_innermost_loop() {
        let src = "total :int = 0;\ni :int = 0;\nwhile (i < 3) {\n    i += 1;\n    inner :int = loop {\n        if (i == 2) {\n            break 100;\n        }\n        break i;\n    };\n    total += inner;\n}\nprint(total);\n";
        assert_eq!(out(src), vec!["104"]);
    }

    #[test]
    fn a_plain_loop_ignores_a_break_value() {
        let src = "i :int = 0;\nloop {\n    i += 1;\n    if (i == 3) {\n        break 99;\n    }\n}\nprint(i);\nloop {\n    if (i == 5) {\n        break;\n    }\n    i += 1;\n}\nprint(i);\n";
        assert_eq!(out(src), vec!["3", "5"]);
    }

    #[test]
    fn a_break_value_leaves_a_try_without_running_finally() {
        // A `break` is a signal in the interpreter, so the `finally` of a `try`
        // it is leaving does not run -- the loop value must skip it too.
        let src = "n :int = loop {\n    စမ်းရန်\n        print(\"body\");\n        break 7;\n    နောက်ဆုံးတွင်\n        print(\"fin\");\n    ပြီး။\n};\nprint(n);\n";
        assert_eq!(out(src), vec!["body", "7"]);
    }

    #[test]
    fn a_break_value_from_a_catch_body_is_the_loop_value() {
        let src = "r :int = loop {\n    စမ်းရန်\n        n :int = \"no\";\n    E ကို ဖမ်းပါ။\n        print(\"caught\");\n        break 2;\n    နောက်ဆုံးတွင်\n        print(\"fin\");\n    ပြီး။\n};\nprint(r);\n";
        assert_eq!(out(src), vec!["caught", "2"]);
    }

    #[test]
    fn a_value_loop_can_continue_and_then_break() {
        let src = "k :int = 0;\nv :int = loop {\n    k += 1;\n    if (k < 3) {\n        continue;\n    }\n    break k;\n};\nprint(v);\nprint(k);\n";
        assert_eq!(out(src), vec!["3", "3"]);
    }

    #[test]
    fn a_pin_loop_value_is_a_constant_afterwards() {
        let src = "pin P :int = loop {\n    break 5;\n};\nprint(P);\nP = 6;\n";
        let err = run_source(src).unwrap_err();
        assert!(err.contains("E104"), "{err}");
    }

    #[test]
    fn a_loop_value_is_checked_against_the_declared_type() {
        let src = "n :str = loop {\n    break 1;\n};\nprint(n);\n";
        let err = run_source(src).unwrap_err();
        assert!(err.contains("E103"), "{err}");
    }

    #[test]
    fn return_outside_a_function_is_e108() {
        let err = run_source("print(\"before\");\nreturn;\n").unwrap_err();
        assert!(err.contains("E108"), "{err}");
        assert!(err.contains("လိုင်း 2"), "{err}");
    }

    /// Sets: literal (a repeated member kept once), membership, iteration and
    /// printing.
    #[test]
    fn sets_compile_and_run() {
        let src = r#"s မှာ {1, 2, 2, 3} ဖြစ်၏။
            s ကို ဖော်ပြပါ။
            (s ထဲ 2) ဖြစ်ရင်
                "has2" ကို ပြပါ။
            မဖြစ်ရင်
                "no2" ကို ပြပါ။
            ပြီး။
            (s ထဲ 9) ဖြစ်ရင်
                "has9" ကို ပြပါ။
            မဖြစ်ရင်
                "no9" ကို ပြပါ။
            ပြီး။
            m သည် တစ်ခုစီ(s)
                m ကို ပြပါ။
            ပြီး။
"#;
        assert_eq!(out(src), vec!["{1, 2, 3}", "has2", "no9", "1", "2", "3"]);
    }

    /// A class: header parameters, `အတု` field writes in the constructor, the
    /// object printed, and both field-read spellings.
    #[test]
    fn classes_construct_objects_with_fields() {
        let src = r#"ပုံသေ Point(x, y) သည်
                လုပ်ငန်း Point()သည်
                    အတု px သည် x ဖြစ်၏။
                    အတု py သည် y ဖြစ်၏။
                ပြီး။
            ပြီး။
            p သည် Point(3, 4) ဖြစ်၏။
            p ကို ဖော်ပြပါ။
            p.px ကို ပြပါ။
            p ၏ py ကို ပြပါ။
"#;
        assert_eq!(out(src), vec!["Point {px: 3, py: 4}", "3", "4"]);
    }

    /// A field that is not there fails with the interpreter's `E082`.
    #[test]
    fn a_missing_field_reports_e082() {
        let err = run_source(
            r#"ပုံသေ P(x) သည်
                လုပ်ငန်း P()သည်
                    အတု a သည် x ဖြစ်၏။
                ပြီး။
            ပြီး။
            p သည် P(1) ဖြစ်၏။
            p.zz ကို ပြပါ။
"#,
        )
        .unwrap_err();
        assert_eq!(err, "E082 လိုင်း 7 တွင် အရာဝတ္ထု ၌ \"zz\" ဆိုသော အကွက် မရှိပါ။");
    }

    /// Sets and classes survive the container round trip unchanged.
    #[test]
    fn sets_and_classes_round_trip_through_bytes() {
        let src = r#"ပုံသေ P(v) သည်
                လုပ်ငန်း P()သည်
                    အတု v သည် v ဖြစ်၏။
                ပြီး။
            ပြီး။
            s မှာ {1, 2, 3} ဖြစ်၏။
            s ကို ဖော်ပြပါ။
            obj သည် P(s) ဖြစ်၏။
            obj ကို ဖော်ပြပါ။
            obj.v ကို ပြပါ။
"#;
        let program = compiled(src);
        assert_eq!(program.classes.len(), 1);
        let bytes = program.encode().expect("encodes");
        let decoded = Program::decode(&bytes).expect("decodes");
        assert_eq!(decoded.classes.len(), 1);
        assert_eq!(decoded.classes[0].params, vec!["v"]);
        assert_eq!(
            run(&decoded),
            vec!["{1, 2, 3}", "P {v: {1, 2, 3}}", "{1, 2, 3}"]
        );
        assert_eq!(run(&decoded), run(&program));
        assert_eq!(
            decoded.encode().expect("re-encodes"),
            bytes,
            "sets and classes encode byte for byte"
        );
    }

    /// Dictionaries: literal, read, write, membership, iteration (keys), a
    /// nested dictionary and printing -- the whole surface the mission lists.
    #[test]
    fn dictionaries_compile_and_run() {
        let src = r#"d မှာ { "a" သည် 1 ဖြစ်၏။ "b" သည် 2 ဖြစ်၏။ } ဖြစ်၏။
            d ကို ဖော်ပြပါ။
            d["b"] ကို ဖော်ပြပါ။
            d["c"] မှာ 3 ဖြစ်၏။
            d ကို ဖော်ပြပါ။
            (d ထဲ "c") ဖြစ်ရင်
                "has-c" ကို ပြပါ။
            မဖြစ်ရင်
                "no-c" ကို ပြပါ။
            ပြီး။
            (d ထဲ "zz") ဖြစ်ရင်
                "has-zz" ကို ပြပါ။
            မဖြစ်ရင်
                "no-zz" ကို ပြပါ။
            ပြီး။
            k သည် တစ်ခုစီ(d)
                k ကို ဖော်ပြပါ။
            ပြီး။
            nested မှာ { "inner" သည် { "x" သည် 10 ဖြစ်၏။ } ဖြစ်၏။ } ဖြစ်၏။
            nested ကို ဖော်ပြပါ။
            inner သည် nested["inner"] ဖြစ်၏။
            inner["x"] ကို ဖော်ပြပါ။
            total သည် 0 ဖြစ်၏။
            key သည် တစ်ခုစီ(d)
                total သည် total + d[key] ဖြစ်၏။
            ပြီး။
            total ကို ဖော်ပြပါ။
"#;
        assert_eq!(
            out(src),
            vec![
                "{\"a\": 1, \"b\": 2}",
                "2",
                "{\"a\": 1, \"b\": 2, \"c\": 3}",
                "has-c",
                "no-zz",
                "a",
                "b",
                "c",
                "{\"inner\": {\"x\": 10}}",
                "10",
                "6",
            ]
        );
    }

    /// A missing key fails with the interpreter's `E053`, key included.
    #[test]
    fn a_missing_dictionary_key_reports_e053() {
        let err =
            run_source("d မှာ { \"a\" သည် 1 ဖြစ်၏။ } ဖြစ်၏။\nd[\"nope\"] ကို ဖော်ပြပါ။\n").unwrap_err();
        assert_eq!(err, "E053 လိုင်း 2 တွင် သော့ \"nope\" ကို ရှာမတွေ့ပါ။");
    }

    /// A dictionary constant survives the container round trip unchanged,
    /// inside a function body as well as at the top level.
    #[test]
    fn dictionaries_round_trip_through_bytes() {
        let src = r#"လုပ်ငန်း show()သည်
                inner မှာ { "a" သည် 1 ဖြစ်၏။ "b" သည် [1, 2] ဖြစ်၏။ } ဖြစ်၏။
                inner ကို ဖော်ပြပါ။
            ပြီး။
            show ကို လုပ်ပါ။
            m မှာ { "n" သည် { "x" သည် 1 ဖြစ်၏။ } ဖြစ်၏။ } ဖြစ်၏။
            m ကို ဖော်ပြပါ။
"#;
        let program = compiled(src);
        let bytes = program.encode().expect("encodes");
        let decoded = Program::decode(&bytes).expect("decodes");
        assert_eq!(run(&decoded), run(&program));
        assert_eq!(
            run(&decoded),
            vec!["{\"a\": 1, \"b\": [1, 2]}", "{\"n\": {\"x\": 1}}"]
        );
        assert_eq!(
            decoded.encode().expect("re-encodes"),
            bytes,
            "the dictionary encodes byte for byte"
        );
    }

    fn compiled(src: &str) -> Program {
        compile_source(src).expect("compiles")
    }

    fn run(program: &Program) -> Vec<String> {
        let mut vm = Vm::new(program);
        vm.run().expect("runs");
        vm.out
    }

    /// A program with a lambda value survives encode -> decode unchanged and
    /// runs the same, so the new call instructions are part of the container.
    #[test]
    fn a_lambda_program_round_trips_through_bytes() {
        let src = r#"fn adder(n) { return fn(x) { return x + n; }; }
add5 = adder(5);
print(add5(1));
print(map([1, 2], add5));
"#;
        let program = compiled(src);
        let bytes = program.encode().expect("encodes");
        let decoded = Program::decode(&bytes).expect("decodes");
        assert_eq!(run(&decoded), vec!["6", "[6, 7]"]);
        assert_eq!(run(&decoded), run(&program));
        assert_eq!(
            decoded.encode().expect("re-encodes"),
            bytes,
            "the lambda instructions encode byte for byte"
        );
    }

    /// A program with a function, a loop and a string constant survives
    /// encode -> decode unchanged, runs the same, and re-encodes byte for byte.
    #[test]
    fn program_round_trips_through_bytes() {
        let src = r#"i သည် တစ်ခုစီ(0, 3)
    i ကို ဖော်ပြပါ။
ပြီး။
လုပ်ငန်း double(n)သည်
    n * 2
ပြီး။
x သည် double(21) ဖြစ်၏။
x ကို ဖော်ပြပါ။
"မြန်မာ" ကို ပြပါ။
"#;
        let program = compiled(src);
        let bytes = program.encode().expect("encodes");
        assert!(bytes.starts_with(&MAGIC), "container magic first");
        let decoded = Program::decode(&bytes).expect("decodes");

        assert_eq!(decoded.funcs.len(), program.funcs.len());
        assert_eq!(decoded.code.len(), program.code.len());
        assert_eq!(run(&decoded), run(&program));
        assert_eq!(
            decoded.encode().expect("re-encodes"),
            bytes,
            "encoding is stable"
        );
    }

    /// A container from another format revision is refused loudly.
    #[test]
    fn a_foreign_format_version_is_refused() {
        let bytes = compiled("\"hi\" ကို ပြပါ။\n").encode().expect("encodes");
        let mut broken = bytes.clone();
        let next = (FORMAT_VERSION + 7).to_le_bytes();
        broken[8..10].copy_from_slice(&next);
        let err = Program::decode(&broken).unwrap_err();
        assert!(err.starts_with("E175"), "{err}");
        assert!(err.contains("ဗားရှင်း"), "{err}");
    }

    /// Half a container is a read error, never a partial program.
    /// A container written by another revision is refused with `E175`, naming
    /// both versions, so a stale artifact never runs with the wrong layout.
    #[test]
    fn a_container_from_another_version_is_refused() {
        let program = compile_source("print(1);\n").expect("compiles");
        let mut bytes = program.encode().expect("encodes");
        bytes[8] = 13;
        bytes[9] = 0;
        let err = Program::decode(&bytes).unwrap_err();
        assert!(err.contains("E175"), "{err}");
        assert!(err.contains("13"), "{err}");
        assert!(err.contains(&FORMAT_VERSION.to_string()), "{err}");
        // Revision 14 (the format before lambdas became values) is stale too.
        let mut bytes = program.encode().expect("encodes");
        bytes[8] = 14;
        bytes[9] = 0;
        let err = Program::decode(&bytes).unwrap_err();
        assert!(err.contains("E175"), "{err}");
        assert!(err.contains("14"), "{err}");
        assert!(err.contains(&FORMAT_VERSION.to_string()), "{err}");
    }

    #[test]
    fn a_truncated_container_is_refused() {
        let bytes = compiled("\"hi\" ကို ပြပါ။\n").encode().expect("encodes");
        let err = Program::decode(&bytes[..bytes.len() / 2]).unwrap_err();
        assert!(err.starts_with("E175"), "{err}");
    }

    // -----------------------------------------------------------------
    // runtime errors: the annotated report, and floor division
    // -----------------------------------------------------------------

    /// A runtime error is drawn like the interpreter's: the message, the
    /// source line, a caret under the problem and the Myanmar label.
    #[test]
    fn a_runtime_error_is_drawn_with_its_source_line() {
        let err = run_source("x :int = \"no\";\n").unwrap_err();
        assert!(err.contains("\n\nline 1| x :int = \"no\";\n"), "{err}");
        let caret = err.lines().nth(3).expect("caret row");
        assert_eq!(
            caret.chars().position(|c| c == '↑'),
            Some("line 1| ".chars().count()),
            "{err}"
        );
        assert!(err.ends_with("အမျိုးအစား မကိုက်ညီ"), "{err}");

        let err = run_source("print(10 // 0);\n").unwrap_err();
        assert!(err.contains("E073"), "{err}");
        assert!(err.contains("\n\nline 1| print(10 // 0);\n"), "{err}");
        assert!(err.ends_with("သုညဖြင့် စား၍မရ"), "{err}");
    }

    /// The report is drawn once: the same message passing through the
    /// function boundary and the run's own boundary is not redrawn.
    #[test]
    fn a_runtime_error_is_never_drawn_twice() {
        let err = run_source("fn f() {\n    z :int = \"no\";\n}\nf();\n").unwrap_err();
        assert_eq!(
            err.matches("\n\nline 2|     z :int = \"no\";\n").count(),
            1,
            "{err}"
        );
        assert!(err.ends_with("အမျိုးအစား မကိုက်ညီ"), "{err}");
    }

    /// A compiled chunk keeps its source in memory for the report, but the
    /// byte container stays instructions-only: a decoded program has no text
    /// to draw, so its error is the plain message.
    #[test]
    fn the_byte_container_carries_no_source_text() {
        let src = "fn f() {\n    z :int = \"no\";\n}\nf();\n";
        let program = compile_source(src).expect("compiles");
        assert_eq!(&*program.funcs[0].source, src, "the chunk keeps its source");
        let bytes = program.encode().expect("encodes");
        assert!(
            !bytes
                .windows(src.len())
                .any(|window| window == src.as_bytes()),
            "the container must not embed the source"
        );
        let decoded = Program::decode(&bytes).expect("decodes");
        assert!(
            decoded.source.is_empty(),
            "decoded top-level source is empty"
        );
        assert!(
            decoded.funcs[0].source.is_empty(),
            "decoded chunk source is empty"
        );
        let mut vm = Vm::new(&decoded);
        let err = vm.run().unwrap_err();
        assert!(err.contains("E103"), "{err}");
        assert!(!err.contains("\n\nline "), "{err}");
    }

    /// The frame's own source is what a body error is drawn against, not the
    /// top-level program's text.
    #[test]
    fn a_function_error_uses_the_functions_source() {
        let main_src = "fn f() {\n    z :int = \"no\";\n}\nf();\n";
        let mut program = compile_source(main_src).expect("compiles");
        program.funcs[0].source = std::rc::Rc::from("first\n    z :int = \"no\";\n");
        let mut vm = Vm::new(&program);
        let err = vm.run().unwrap_err();
        assert!(err.contains("\n\nline 2|     z :int = \"no\";\n"), "{err}");
        assert!(!err.contains("fn f()"), "{err}");
    }

    /// `//` runs on the VM with the interpreter's values and errors.
    #[test]
    fn floor_division_matches_the_interpreter() {
        assert_eq!(
            out("print(10 // 3);\nprint(10 // 3.0);\nprint(-7 // 2);\n"),
            vec!["3", "3.0", "-4"]
        );
        let err = run_source("print(10 // 0);\n").unwrap_err();
        assert!(err.contains("E073"), "{err}");
        let err = run_source("print(10 // 0.0);\n").unwrap_err();
        assert!(err.contains("E074"), "{err}");
    }

    /// Junk after the top-level chunk is rejected rather than ignored.
    #[test]
    fn trailing_bytes_are_refused() {
        let mut bytes = compiled("\"hi\" ကို ပြပါ။\n").encode().expect("encodes");
        bytes.push(0xAA);
        let err = Program::decode(&bytes).unwrap_err();
        assert!(err.contains("အပို"), "{err}");
    }

    /// A script library's own source is what its errors are drawn against:
    /// the library's line and caret, not the importing file's.
    fn library_program(tag: &str, lib: &str, main: &str) -> Result<Program, String> {
        library_program_files(tag, &[("LibA.akk", lib)], main)
    }

    fn library_program_files(
        tag: &str,
        files: &[(&str, &str)],
        main: &str,
    ) -> Result<Program, String> {
        use crate::project::library::LibraryLoader;
        let dir = std::env::temp_dir().join(format!("akk-vm-lib-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        for (name, body) in files {
            std::fs::write(dir.join(name), body).expect("write library");
        }
        let tokens = crate::frontend::lexer::lex(main)?;
        let stmts = crate::frontend::parser::parse(&tokens)?;
        let mut loader = LibraryLoader::new(dir.join("libraries"));
        loader.add_script_dir(dir);
        compile_with_libraries(&stmts, main, &mut loader)
    }

    #[test]
    fn a_library_error_is_drawn_from_the_library_source_once() {
        const LIB: &str =
            "print(\"lib loaded\");\n\nfn boom(x) {\n    y :int = \"no\";\n    return y;\n}\n";
        let main = "နည်းပညာများ LibA ကို သုံးမည်။\nLibA ၏ boom(1) ကို လုပ်ပါ။\n";
        let program = library_program("attr", LIB, main).expect("the library compiles in");
        assert_eq!(
            program.script_libs.len(),
            1,
            "the library's init function is registered"
        );
        let mut vm = Vm::new(&program);
        let err = vm.run().unwrap_err();
        assert!(err.contains("E103"), "{err}");
        // Line 4 is the library's own line, shown with the library's text.
        assert!(err.contains("\n\nline 4|     y :int = \"no\";\n"), "{err}");
        assert!(!err.contains("print(\"lib loaded\");\n\nline"), "{err}");
        assert!(err.ends_with("အမျိုးအစား မကိုက်ညီ"), "{err}");
        assert_eq!(err.matches("\n\nline ").count(), 1, "{err}");
    }

    /// A library error in the importing file's `try` is caught there, and its
    /// snippet is not drawn at all.
    #[test]
    fn a_library_error_is_caught_in_the_importing_file() {
        const LIB: &str = "print(\"lib loaded\");\n\nfn boom() {\n    y :int = \"no\";\n}\n";
        let main = "နည်းပညာများ LibA ကို သုံးမည်။\nစမ်းရန်\n    LibA ၏ boom() ကို လုပ်ပါ။\nE103 ကို ဖမ်းပါ။\n    print(\"caught\");\nပြီး။\n";
        let program = library_program("catch", LIB, main).expect("the library compiles in");
        let mut vm = Vm::new(&program);
        vm.run().expect("the handler catches the library error");
        assert_eq!(vm.out, vec!["lib loaded", "caught"]);
    }

    /// A library's top-level statements run once, at the import, in the
    /// library's own source.
    #[test]
    fn a_library_toplevel_error_is_drawn_from_the_library_source() {
        const LIB: &str = "z :int = \"no\";\n";
        let main = "နည်းပညာများ LibA ကို သုံးမည်။\n";
        let program = library_program("top", LIB, main).expect("the library compiles in");
        let mut vm = Vm::new(&program);
        let err = vm.run().unwrap_err();
        assert!(err.contains("E103"), "{err}");
        assert!(err.contains("\n\nline 1| z :int = \"no\";\n"), "{err}");
        assert_eq!(err.matches("\n\nline ").count(), 1, "{err}");
    }

    /// A library that imports itself fails with the interpreter's `E119` at
    /// the import line in the library's own source, drawn once.
    #[test]
    fn a_self_importing_library_fails_with_e119_from_its_own_line() {
        const LIB: &str = "print(\"lib top\");\nနည်းပညာများ LibA ကို သုံးမည်။\n";
        let main = "နည်းပညာများ LibA ကို သုံးမည်။\n";
        let program = library_program("cycle-self", LIB, main).expect("it compiles");
        let mut vm = Vm::new(&program);
        let err = vm.run().unwrap_err();
        assert!(err.contains("E119"), "{err}");
        assert!(
            err.contains("\n\nline 2| နည်းပညာများ LibA ကို သုံးမည်။\n"),
            "{err}"
        );
        assert!(err.ends_with("အပြန်အလှန် သွင်းယူမှု"), "{err}");
        assert_eq!(err.matches("\n\nline ").count(), 1, "{err}");
        // The library's top-level statements before the cycle ran once.
        assert_eq!(vm.out, vec!["lib top"]);
    }

    /// Two libraries that import each other fail where the cycle closes: the
    /// second library's import line, in its own source.
    #[test]
    fn a_mutual_library_cycle_fails_with_e119_from_the_closing_library() {
        const M1: &str = "print(\"M1 top\");\nနည်းပညာများ LibB ကို သုံးမည်။\n";
        const M2: &str = "print(\"M2 top\");\nနည်းပညာများ LibA ကို သုံးမည်။\n";
        let main = "နည်းပညာများ LibA ကို သုံးမည်။\n";
        let program =
            library_program_files("cycle-mutual", &[("LibA.akk", M1), ("LibB.akk", M2)], main)
                .expect("it compiles");
        let mut vm = Vm::new(&program);
        let err = vm.run().unwrap_err();
        assert!(err.contains("E119"), "{err}");
        assert!(
            err.contains("\n\nline 2| နည်းပညာများ LibA ကို သုံးမည်။\n"),
            "{err}"
        );
        assert_eq!(err.matches("\n\nline ").count(), 1, "{err}");
        assert_eq!(vm.out, vec!["M1 top", "M2 top"]);
    }

    /// An acyclic transitive import runs each library's top-level statements
    /// exactly once, however many times the program imports them.
    #[test]
    fn a_transitive_import_runs_each_library_once() {
        const A: &str = "print(\"A top\");\nနည်းပညာများ ChainB ကို သုံးမည်။\nfn a() { return \"a\"; }\n";
        const B: &str = "print(\"B top\");\nနည်းပညာများ ChainC ကို သုံးမည်။\nfn b() { return \"b\"; }\n";
        const C: &str = "print(\"C top\");\nfn c() { return \"c\"; }\n";
        let main =
            "နည်းပညာများ ChainA ကို သုံးမည်။\nနည်းပညာများ ChainB ကို သုံးမည်။\nprint(a() + b() + c());\n";
        let program = library_program_files(
            "chain-once",
            &[("ChainA.akk", A), ("ChainB.akk", B), ("ChainC.akk", C)],
            main,
        )
        .expect("it compiles");
        let mut vm = Vm::new(&program);
        vm.run().expect("the acyclic chain runs");
        assert_eq!(vm.out, vec!["A top", "B top", "C top", "abc"]);
    }

    /// Compiling a library in keeps its text in the chunks (for reports) but
    /// never in the byte container, and a decoded container has no library.
    #[test]
    fn a_compiled_library_is_not_embedded_in_the_byte_container() {
        const LIB: &str = "# LIBRARY_SOURCE_SENTINEL_9911\nfn boom() {\n    y :int = \"no\";\n}\n";
        let main = "နည်းပညာများ LibA ကို သုံးမည်။\nLibA ၏ boom() ကို လုပ်ပါ။\n";
        let program = library_program("bytes", LIB, main).expect("the library compiles in");
        let bytes = program.encode().expect("encodes");
        let sentinel = b"LIBRARY_SOURCE_SENTINEL_9911";
        assert!(
            !bytes.windows(sentinel.len()).any(|w| w == sentinel),
            "the container must not embed library source"
        );
        let decoded = Program::decode(&bytes).expect("decodes");
        assert!(
            decoded.script_libs.is_empty(),
            "a decoded container has no library init functions"
        );
    }
}
