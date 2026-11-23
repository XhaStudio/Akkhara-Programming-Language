//! End-to-end tests for `akk build <file>`: the native code generator.
//!
//! Each case compiles a program from `tests/native/fixtures/`, runs the
//! executable that `akk build` wrote, and checks that it prints exactly what
//! the interpreter prints for the same file and exits with the same status.
//! Every case runs twice: once through the optimiser and once with
//! `--no-opt`, so the slice cannot quietly depend on `opt`.
//!
//! The programs are built and run on this machine, so the test only means
//! something on a platform the backend writes for (`x86_64` Linux or Windows).
//! On anything else it says so and skips.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// One fixture and the exact stdout it must produce.
struct Case {
    /// `tests/native/fixtures/<name>.akk`
    name: &'static str,
    stdout: &'static [&'static str],
}

const CASES: &[Case] = &[
    // Arithmetic that has to be computed: `6 * 7`, then `- 2`, then `* 3`,
    // then `% 40`, then `9 - 5`.
    Case {
        name: "arith",
        stdout: &["42", "40", "120", "0", "4"],
    },
    // A comparison decides which print runs, twice, then a bool is printed.
    // `%` on ints and on floats: the interpreter's `rem_euclid`, promoted to
    // a float when either side is one, and through a function and `%=`.
    Case {
        name: "remainder",
        stdout: &[
            "1", "2", "1", "1.5", "0.5", "1.5", "0.5", "2.0", "1.5", "0.0", "1.25", "2.75", "1.5",
            "1",
        ],
    },
    Case {
        name: "branch",
        stdout: &["big", "seven", "False"],
    },
    // Several literals, each printed on its own line by the runtime.
    Case {
        name: "strings",
        stdout: &["first", "second line", "third", "end"],
    },
    // String comparison: equality and lexicographic ordering, including a
    // proper prefix (the shorter string is the smaller one).
    Case {
        name: "strcmp",
        stdout: &[
            "True", "False", "True", "True", "True", "True", "True", "True", "True",
        ],
    },
    // The numeric builtins: `abs` of an int and of a float, `min`/`max` over
    // several candidates, one candidate, and a float `min` reached through a
    // function's parameters.
    Case {
        name: "builtins",
        stdout: &["7", "2.5", "1", "3", "2.5", "-2.5", "4", "1.5"],
    },
    // Collection literals: a list indexed and printed, `len`, a dict looked
    // up by key, set deduplication, a tuple, a nested list and a float list.
    // The interpreter is the reference for every line.
    Case {
        name: "collections",
        stdout: &[
            "20",
            "[10, 20, 30]",
            "3",
            "2",
            "{\"a\": 1, \"b\": 2}",
            "{1, 2, 3}",
            "(1, 2, 3)",
            "[[1, 2], [3]]",
            "[1.5, 2.0]",
            "[True, False]",
            "[\"a\", \"b\"]",
        ],
    },
    // Collection equality: the same elements compare equal, a different
    // length/order/kind or one different element does not, and a collection
    // never equals a value of another type.
    Case {
        name: "equality",
        stdout: &[
            "True",
            "False",
            "True",
            "False",
            "True",
            "False",
            "True",
            "False",
            "True",
            "False",
            "False",
            "True",
            "True",
            "False",
            "True",
            "False",
            "True",
            "False",
            "False",
            "True",
            "False",
            "same",
            "different",
        ],
    },
    // Writing through an index: list/tuple replacement and append, set
    // replacement and its duplicate rule, dict update and new key, and the
    // value semantics of a second binding.
    Case {
        name: "mutation",
        stdout: &[
            "[10, 99, 30]",
            "[10, 99, 30, 40]",
            "4",
            "40",
            "(7, 2, 3)",
            "(7, 2, 3, 4)",
            "{1, 9, 3}",
            "{9, 3}",
            "{9, 3}",
            "{\"a\": 1, \"b\": 20, \"c\": 3}",
            "3",
            "[10, 99, 30, 40]",
            "[1, 99, 30, 40]",
        ],
    },
    // Strings whose text is known at compile time: `len` of a literal, an
    // ASCII and a Myanmar syllable count, `for` over the grapheme clusters, an
    // index read (literal and computed), and an empty string, whose loop runs
    // no iterations at all.
    Case {
        name: "strings_ops",
        stdout: &[
            "5", "3", "0", "3", "မြ", "န်", "မာ", "မြ", "မာ", "န်", "e", "5",
        ],
    },
    // `+` joins two strings: constant pairs, a run-time pair from bindings, a
    // nested join, a parameter pair, a binding growing through `+`, and the
    // empty string (which prints as an empty line).
    Case {
        name: "concat",
        stdout: &[
            "foobar",
            "မြန်မာ",
            "",
            "abc",
            "abcd",
            "cdab",
            "True",
            "hello world!",
            "xyz",
        ],
    },
    // The interpreter's auto-range: a numeric source walks the ints below it,
    // from a binding, a float bound (rounded up, so 2.5 runs three times), a
    // zero and a negative bound (no iterations), and a parameter.
    Case {
        name: "number_for",
        stdout: &["0", "10", "20", "0", "1", "2", "100", "101", "done"],
    },
    // A collection's element shape crosses a call: a list, tuple, nested list
    // and dict passed as parameters, a collection returned from a function,
    // the result of one call indexed by the next, and iteration in the callee.
    Case {
        name: "collection_params",
        stdout: &["10", "5", "3", "[9, 8]", "2", "a", "11", "5", "4", "5"],
    },
    // `for` over a list, a tuple, a set and a dict (whose iteration yields its
    // keys), then `continue` and `break` inside a collection loop.
    Case {
        name: "iteration",
        stdout: &["10", "20", "30", "1", "2", "3", "a", "b", "a", "b", "10"],
    },
    // `floor`, `ceil` and `round`: positive, negative, exact half, an int
    // argument, an int beyond 2^53 (the interpreter converts to a double
    // first), and one reached through a function's parameter.
    Case {
        name: "rounding",
        stdout: &[
            "3",
            "4",
            "3",
            "-3",
            "-3",
            "-2",
            "2",
            "7",
            "9007199254740992",
            "0",
            "1",
            "-1",
            "2",
        ],
    },
    // `add(a, b)` called directly, called through `twice(x)`, and called as
    // part of an expression whose other operand is another call.
    Case {
        name: "calls",
        stdout: &["13", "12", "7"],
    },
    // Self-recursion where each call needs its own frame: `factorial(10)`
    // nests ten deep and `fib(15)` fans out into hundreds of calls.
    Case {
        name: "recursion",
        stdout: &["3628800", "610"],
    },
    // Mutual recursion: `is_even` and `is_odd` call each other, so a call to
    // a function defined later in the file has to work.
    Case {
        name: "mutual",
        stdout: &["1", "0", "0"],
    },
    // Three locals in one frame (`sum`, `diff`, `prod`) with prints between
    // them and around the call, so nothing survives by luck in a register.
    Case {
        name: "locals",
        stdout: &["before", "13", "65", "65", "after"],
    },
    // Arguments that are themselves calls, nested two deep, plus a callee that
    // prints while its argument is being evaluated (`noisy(1)` before
    // `noisy(2)`, left to right as the interpreter does it).
    Case {
        name: "nested",
        stdout: &["22", "9", "1", "2", "3"],
    },
    // Float literals, a zero and a negated float, printed with the
    // interpreter's exact spelling (`0.30000000000000004`, not `0.3`).
    Case {
        name: "floats",
        stdout: &["1.5", "0.0", "-2.25", "0.30000000000000004"],
    },
    // True division and mixed int/float arithmetic: `7 / 2` must be `3.5`
    // rather than a truncated `3`, and the compound assignments stay float.
    Case {
        name: "floatmath",
        stdout: &[
            "3.5",
            "0.3333333333333333",
            "6.0",
            "3.5",
            "4.0",
            "0.25",
            "3.3333333333333335",
            "1.5",
        ],
    },
    // Ordering promotes an int, but `==` keeps the interpreter's rule that an
    // int and a float are never equal, and `0.1 + 0.2` is not `0.3`.
    Case {
        name: "floatcmp",
        stdout: &["True", "False", "False", "True", "True", "False"],
    },
    // Floats through parameters and returns, a call nested in a call's
    // argument, and an int argument promoted at the call site.
    Case {
        name: "floatfn",
        stdout: &["3.0", "4.5", "1.5"],
    },
    // Raising and catching: an identity clause, `E178` (every raise), `E`
    // (anything), and a raise from inside a function that crosses a clause
    // which does not match it -- running that clause's finally first -- before
    // the caller's own clause takes it.
    Case {
        name: "raise",
        stdout: &[
            "start",
            "caught by identity",
            "insufficient",
            "identity finally",
            "caught by E178",
            "second",
            "caught by E",
            "third",
            "inner finally",
            "outer caught",
            "negative",
            "plain body",
            "plain finally",
            "end",
        ],
    },
];

fn akk() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_akk"))
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("native")
        .join("fixtures")
}

/// A clean directory per case and build mode, so the interpreter's `.akop`
/// and the executable from one run cannot leak into another.
fn work_dir(name: &str, label: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("native-tests")
        .join(format!("{name}-{label}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("a work directory");
    dir
}

fn text(bytes: &[u8]) -> String {
    // Normalize CRLF so the expectations hold on every platform.
    String::from_utf8_lossy(bytes).replace("\r\n", "\n")
}

/// Drops the `Compiled in ...` line the interpreter prints when it has to
/// build, so only the program's own output is compared.
fn program_output(stdout: String) -> String {
    let lines: Vec<&str> = stdout
        .lines()
        .filter(|line| !line.starts_with("Compiled in "))
        .collect();
    if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    }
}

fn expected(lines: &[&str]) -> String {
    if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    }
}

/// The path `akk build` writes for `source`: beside it, with the platform's
/// executable suffix.
fn executable_for(source: &Path) -> PathBuf {
    let mut name = source
        .file_stem()
        .map(|stem| stem.to_os_string())
        .unwrap_or_else(|| std::ffi::OsString::from("program"));
    name.push(std::env::consts::EXE_SUFFIX);
    source.with_file_name(name)
}

fn run(command: &mut Command) -> Output {
    command
        .output()
        .unwrap_or_else(|error| panic!("failed to run {:?}: {error}", command))
}

/// `akk <path>`: the interpreter, which is the reference for every case.
fn interpret(source: &Path) -> Output {
    run(&mut Command::new(akk()).arg(source))
}

/// `akk build <path>` (plus `--no-opt`), which must write the executable.
fn build(source: &Path, optimize: bool) -> Output {
    let mut command = Command::new(akk());
    command.arg("build").arg(source);
    if !optimize {
        command.arg("--no-opt");
    }
    run(&mut command)
}

fn execute(program: &Path) -> Output {
    run(&mut Command::new(program))
}

#[test]
fn built_programs_match_the_interpreter() {
    if !cfg!(target_arch = "x86_64") {
        eprintln!("skipping: the native backend writes x86_64 only");
        return;
    }
    let mut failures = Vec::new();
    for case in CASES {
        for optimize in [true, false] {
            let label = if optimize { "opt" } else { "no-opt" };
            let name = format!("{} ({label})", case.name);
            let dir = work_dir(case.name, label);
            let source = dir.join(format!("{}.akk", case.name));
            fs::copy(fixtures().join(format!("{}.akk", case.name)), &source).expect("the fixture");

            let reference = interpret(&source);
            let reference_stdout = program_output(text(&reference.stdout));
            let reference_exit = reference.status.code().unwrap_or(-1);

            let built = build(&source, optimize);
            if !built.status.success() {
                failures.push(format!(
                    "{name}: `akk build` failed with {}\n    stderr: {}",
                    built.status,
                    text(&built.stderr).trim()
                ));
                continue;
            }

            let program = executable_for(&source);
            if !program.exists() {
                failures.push(format!(
                    "{name}: no executable was written to {}",
                    program.display()
                ));
                continue;
            }

            let native = execute(&program);
            let native_stdout = text(&native.stdout);
            let native_exit = native.status.code().unwrap_or(-1);

            let want = expected(case.stdout);
            if native_stdout != want {
                failures.push(format!(
                    "{name}: stdout was {native_stdout:?}, expected {want:?}"
                ));
            }
            if native_stdout != reference_stdout {
                failures.push(format!(
                    "{name}: the executable and the interpreter disagree\n    executable: {native_stdout:?}\n    interpreter: {reference_stdout:?}"
                ));
            }
            if native_exit != reference_exit {
                failures.push(format!(
                    "{name}: exit code {native_exit}, the interpreter exited {reference_exit}"
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} native check(s) failed:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

/// `%` by zero is the one run-time failure the slice can hit. The interpreter
/// reports `E038` on stderr and exits 1; the executable writes the same
/// message to stderr (with no line number, because compiled code carries no
/// spans) and exits 1 too.
#[test]
fn a_modulo_by_zero_panics_like_the_interpreter() {
    if !cfg!(target_arch = "x86_64") {
        return;
    }
    let dir = work_dir("mod_zero", "opt");
    let source = dir.join("mod_zero.akk");
    fs::copy(fixtures().join("mod_zero.akk"), &source).expect("the fixture");

    let reference = interpret(&source);
    let built = build(&source, true);
    assert!(
        built.status.success(),
        "`akk build` failed: {}",
        text(&built.stderr).trim()
    );
    let native = execute(&executable_for(&source));

    assert_eq!(
        program_output(text(&reference.stdout)),
        "before\n",
        "the interpreter prints the statement before the panic"
    );
    assert_eq!(
        text(&native.stdout),
        "before\n",
        "the executable prints the statement before the panic"
    );
    let reference_stderr = text(&reference.stderr);
    let native_stderr = text(&native.stderr);
    assert!(
        reference_stderr.contains("E038"),
        "the interpreter should report E038, got {reference_stderr:?}"
    );
    assert!(
        native_stderr.starts_with("E038"),
        "the executable should reach the panic, got {native_stderr:?}"
    );
    assert_eq!(
        native.status.code(),
        reference.status.code(),
        "exit codes should match (stderr: {})",
        text(&native.stderr).trim()
    );
    assert_eq!(native.status.code(), Some(1));
}

/// The interpreter reports `E037` for a float divided by zero, and so does the
/// executable: the divisor is checked before the divide, so the hardware never
/// produces an infinity the interpreter would not print.
#[test]
fn a_float_division_by_zero_panics_like_the_interpreter() {
    if !cfg!(target_arch = "x86_64") {
        return;
    }
    let dir = work_dir("float_div_zero", "opt");
    let source = dir.join("float_div_zero.akk");
    fs::copy(fixtures().join("float_div_zero.akk"), &source).expect("the fixture");

    let reference = interpret(&source);
    let built = build(&source, true);
    assert!(
        built.status.success(),
        "`akk build` failed: {}",
        text(&built.stderr).trim()
    );
    let native = execute(&executable_for(&source));

    assert_eq!(
        program_output(text(&reference.stdout)),
        "before\n",
        "the interpreter prints the statement before the panic"
    );
    assert_eq!(
        text(&native.stdout),
        "before\n",
        "the executable prints the statement before the panic"
    );
    let reference_stderr = text(&reference.stderr);
    let native_stderr = text(&native.stderr);
    assert!(
        reference_stderr.contains("E037"),
        "the interpreter should report E037, got {reference_stderr:?}"
    );
    assert!(
        native_stderr.starts_with("E037"),
        "the executable should reach the panic, got {native_stderr:?}"
    );
    assert_eq!(native.status.code(), Some(1));
    assert_eq!(native.status.code(), reference.status.code());
}

/// A float remainder by zero is the interpreter's `E039`: the executable writes
/// the same message (without the line number compiled code has no span for) to
/// stderr and exits 1.
#[test]
fn a_float_remainder_by_zero_panics_like_the_interpreter() {
    if !cfg!(target_arch = "x86_64") {
        return;
    }
    let dir = work_dir("float_rem_zero", "opt");
    let source = dir.join("float_rem_zero.akk");
    fs::copy(fixtures().join("float_rem_zero.akk"), &source).expect("the fixture");

    let reference = interpret(&source);
    let built = build(&source, true);
    assert!(
        built.status.success(),
        "`akk build` failed: {}",
        text(&built.stderr).trim()
    );
    let native = execute(&executable_for(&source));

    assert_eq!(
        program_output(text(&reference.stdout)),
        "before\n",
        "the interpreter prints the statement before the panic"
    );
    assert_eq!(
        text(&native.stdout),
        "before\n",
        "the executable prints the statement before the panic"
    );
    let reference_stderr = text(&reference.stderr);
    let native_stderr = text(&native.stderr);
    assert!(
        reference_stderr.contains("E039"),
        "the interpreter should report E039, got {reference_stderr:?}"
    );
    assert!(
        native_stderr.starts_with("E039"),
        "the executable should reach the panic, got {native_stderr:?}"
    );
    assert_eq!(native.status.code(), Some(1));
    assert_eq!(native.status.code(), reference.status.code());
}

/// A raise that reaches the top level terminates the built program the way it
/// terminates the interpreter: the `E178` line -- with its line number and its
/// identity -- on stderr, and exit 1.
#[test]
fn an_uncaught_raise_reports_like_the_interpreter() {
    if !cfg!(target_arch = "x86_64") {
        return;
    }
    for optimize in [true, false] {
        let label = if optimize { "opt" } else { "no-opt" };
        let dir = work_dir("raise_uncaught", label);
        let source = dir.join("raise_uncaught.akk");
        fs::copy(fixtures().join("raise_uncaught.akk"), &source).expect("the fixture");

        let reference = interpret(&source);
        let built = build(&source, optimize);
        assert!(
            built.status.success(),
            "`akk build` ({label}) failed: {}",
            text(&built.stderr).trim()
        );
        let native = execute(&executable_for(&source));

        assert_eq!(
            program_output(text(&reference.stdout)),
            "before\n",
            "the interpreter prints the statement before the raise"
        );
        assert_eq!(
            text(&native.stdout),
            "before\n",
            "the built program prints the statement before the raise"
        );
        let reference_stderr = text(&reference.stderr);
        let native_stderr = text(&native.stderr);
        assert!(
            reference_stderr.contains("E178 လိုင်း 2 တွင် [Broken] boom"),
            "the interpreter should report the raise, got {reference_stderr:?}"
        );
        assert_eq!(
            native_stderr, reference_stderr,
            "the built program and the interpreter should put the same line on stderr"
        );
        assert_eq!(
            native.status.code(),
            reference.status.code(),
            "exit codes should match (stderr: {})",
            native_stderr.trim()
        );
        assert_eq!(native.status.code(), Some(1));
    }
}

/// A construct is outside the slice, with the word the message should name it
/// by. The program itself is valid: the interpreter runs it before `akk build`
/// is asked to, so the refusal is about the slice and not about the syntax.
struct Refused {
    name: &'static str,
    /// What `E177` should mention.
    mentions: &'static str,
    source: &'static str,
}

const REFUSED: &[Refused] = &[
    // A collection with two element kinds needs the dynamic value word; this
    // slice writes one tag per collection and refuses a mixed literal.
    Refused {
        name: "list_mixed",
        mentions: "list",
        source: "xs မှာ [1, \"a\"] ဖြစ်၏။\nxs ကို ဖော်ပြပါ။\n",
    },
    // A set drops duplicates, so its members must be known well enough to
    // compare; a member computed at run time is still refused.
    Refused {
        name: "set_runtime",
        mentions: "set built from run-time",
        source: "n သည် 1 ဖြစ်၏။\ns မှာ {n, 2} ဖြစ်၏။\ns ကို ဖော်ပြပါ။\n",
    },
    // A dict key has to be a literal so that duplicate keys keep the
    // interpreter's first position / last value rule.
    Refused {
        name: "dict_key",
        mentions: "dict",
        source: "k သည် \"a\" ဖြစ်၏။\nd မှာ { k သည် 1 ဖြစ်၏။ } ဖြစ်၏။\nd ကို ဖော်ပြပါ။\n",
    },
    // A string known at compile time is lowered to a list of grapheme
    // clusters, but one that arrives at run time cannot be: the native
    // runtime carries no Unicode tables, so iteration, `len` and indexing
    // over a parameter are refused by name instead of counting bytes.
    Refused {
        name: "string_iteration",
        mentions: "iteration over a string",
        source: "fn show(s) {\n    for ch in s {\n        print(ch);\n    }\n}\nshow(\"abc\");\n",
    },
    Refused {
        name: "string_len",
        mentions: "len() of a string",
        source: "fn size(s) {\n    return len(s);\n}\nprint(size(\"abc\"));\n",
    },
    Refused {
        name: "string_index",
        mentions: "an index into a string",
        source: "fn second(s) {\n    return s[1];\n}\nprint(second(\"abc\"));\n",
    },
    // A write must match the collection's element kind: the interpreter
    // allows a heterogeneous list, the native layout writes one element kind.
    // A nested element has a shape too: a list of int lists cannot take a
    // list of strings, because the inner reads are typed statically.
    Refused {
        name: "nested_store_kind",
        mentions: "a store of a list of string",
        source: "grid မှာ [[1], [2]] ဖြစ်၏။\ngrid[0] က [\"nine\"]။\n",
    },
    // An empty inner list has no element kind, so reading through it again is
    // not typed; the interpreter would answer at run time instead.
    Refused {
        name: "nested_unknown_inner",
        mentions: "could not be worked out",
        source: "xs မှာ [[]] ဖြစ်၏။\nxs[0] က [7]။\nprint(xs[0][0]);\n",
    },
    Refused {
        name: "mixed_store",
        mentions: "where a int is needed",
        source: "xs မှာ [1, 2] ဖြစ်၏။\nxs[0] က \"a\"။\nxs ကို ဖော်ပြပါ။\n",
    },
    // One function's parameter sees one element shape: a caller passing a
    // list of strings and another passing a list of ints cannot both describe
    // the same statically typed parameter.
    Refused {
        name: "collection_param_mismatch",
        mentions: "where its parameter #1 is a",
        source: "fn pick(xs) {\n    return xs[0];\n}\nprint(pick([\"a\", \"b\"]));\nprint(pick([1, 2]));\n",
    },
    // `min`/`max` take one numeric kind in the slice: the interpreter would
    // compare across int and float, but then the result's kind depends on the
    // values.
    Refused {
        name: "min_mixed",
        mentions: "min",
        source: "m သည် min(1, 2.5) ဖြစ်၏။\nm ကို ဖော်ပြပါ။\n",
    },
    // A compiled program raises only the errors it writes; a clause for one
    // of the interpreter's own codes could never match, so it is refused
    // instead of building a program that silently differs.
    Refused {
        name: "catch_code",
        mentions: "E036",
        source: "စမ်းရန်\n    print(\"ok\");\nE036 ကို ဖမ်းပါ။\n    print(\"never\");\nပြီး။\n",
    },
    Refused {
        name: "closure",
        mentions: "closure",
        source: "fn outer() {\n    n သည် 1 ဖြစ်၏။\n    fn inner() {\n        return n;\n    }\n    return inner();\n}\nouter() ကို ဖော်ပြပါ။\n",
    },
    Refused {
        name: "class",
        mentions: "E177",
        source: "ပုံသေ Point သည်\n    fn sum() {\n        return 1;\n    }\nပြီး။\np သည် Point() ဖြစ်၏။\np ကို ဖော်ပြပါ။\n",
    },
];

/// A builtin called with the wrong kind or number of arguments is a broken
/// program, not a construct outside the slice: the interpreter reports `E110`
/// and `akk build` refuses it with the same code (and the interpreter's
/// wording), only without the line number compiled code has no span for.
struct Misuse {
    name: &'static str,
    /// What the refusal should mention.
    mentions: &'static str,
    source: &'static str,
}

const MISUSE: &[Misuse] = &[
    Misuse {
        name: "floor_kind",
        mentions: "floor",
        source: "x သည် floor(\"a\") ဖြစ်၏။\nx ကို ဖော်ပြပါ။\n",
    },
    Misuse {
        name: "round_arity",
        mentions: "round",
        source: "x သည် round() ဖြစ်၏။\nx ကို ဖော်ပြပါ။\n",
    },
    Misuse {
        name: "abs_kind",
        mentions: "abs",
        source: "x သည် abs(\"a\") ဖြစ်၏။\nx ကို ဖော်ပြပါ။\n",
    },
    Misuse {
        name: "min_arity",
        mentions: "min",
        source: "x သည် min() ဖြစ်၏။\nx ကို ဖော်ပြပါ။\n",
    },
];

/// Both sides must reject each program with `E110`, in both build modes, and
/// no executable may be left behind.
#[test]
fn builtin_misuse_reports_the_interpreters_code() {
    let mut failures = Vec::new();
    for case in MISUSE {
        for optimize in [true, false] {
            let label = if optimize { "opt" } else { "no-opt" };
            let name = format!("{} ({label})", case.name);
            let dir = work_dir(case.name, label);
            let source = dir.join(format!("{}.akk", case.name));
            fs::write(&source, case.source).expect("the source");

            let reference = interpret(&source);
            let reference_stderr = text(&reference.stderr);
            if reference.status.code() != Some(1) || !reference_stderr.contains("E110") {
                failures.push(format!(
                    "{name}: the interpreter should exit 1 with E110, got {}\n    stderr: {}",
                    reference.status,
                    reference_stderr.trim()
                ));
                continue;
            }

            let built = build(&source, optimize);
            if built.status.code() != Some(1) {
                failures.push(format!(
                    "{name}: `akk build` should exit 1, it exited {}",
                    built.status
                ));
            }
            let stderr = text(&built.stderr);
            if !stderr.contains("E110") {
                failures.push(format!(
                    "{name}: stderr should carry the interpreter's E110, was {stderr:?}"
                ));
            }
            if !stderr.contains(case.mentions) {
                failures.push(format!(
                    "{name}: stderr should mention {:?}, was {stderr:?}",
                    case.mentions
                ));
            }
            let program = executable_for(&source);
            if program.exists() {
                failures.push(format!(
                    "{name}: a refused program wrote {}",
                    program.display()
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} builtin-misuse check(s) failed:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

/// Everything outside the slice must fail with `E177` naming it, in both build
/// modes, and must not leave an executable behind that would silently mean
/// something else. The interpreter runs each program first, so the failure is
/// the slice's and not the parser's.
#[test]
fn unsupported_constructs_are_refused_by_name() {
    let mut failures = Vec::new();
    for case in REFUSED {
        for optimize in [true, false] {
            let label = if optimize { "opt" } else { "no-opt" };
            let name = format!("{} ({label})", case.name);
            let dir = work_dir(case.name, label);
            let source = dir.join(format!("{}.akk", case.name));
            fs::write(&source, case.source).expect("the source");

            let reference = interpret(&source);
            if !reference.status.success() {
                failures.push(format!(
                    "{name}: the interpreter should accept this program, it exited {}\n    stderr: {}",
                    reference.status,
                    text(&reference.stderr).trim()
                ));
                continue;
            }

            let built = build(&source, optimize);
            if built.status.code() != Some(1) {
                failures.push(format!(
                    "{name}: `akk build` should exit 1, it exited {}",
                    built.status
                ));
            }
            let stderr = text(&built.stderr);
            if !stderr.contains("E177") {
                failures.push(format!("{name}: stderr should carry E177, was {stderr:?}"));
            }
            if !stderr.contains(case.mentions) {
                failures.push(format!(
                    "{name}: stderr should mention {:?}, was {stderr:?}",
                    case.mentions
                ));
            }
            let program = executable_for(&source);
            if program.exists() {
                failures.push(format!(
                    "{name}: a refused program wrote {}",
                    program.display()
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} refusal check(s) failed:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

/// The collection runtime's `E049` (index out of range) and `E053` (missing
/// dict key) must read exactly like the interpreter's, including the source
/// line, in both build modes.
#[test]
fn collection_diagnostics_match_the_interpreter() {
    if !cfg!(target_arch = "x86_64") {
        return;
    }
    let cases = [
        ("index_range", "xs မှာ [1, 2] ဖြစ်၏။\nxs[5] ကို ဖော်ပြပါ။\n"),
        ("index_negative", "xs မှာ [1, 2] ဖြစ်၏။\nxs[-1] ကို ဖော်ပြပါ။\n"),
        (
            "dict_missing",
            "d မှာ { \"a\" သည် 1 ဖြစ်၏။ } ဖြစ်၏။\nd[\"z\"] ကို ဖော်ပြပါ။\n",
        ),
        (
            "order_collections",
            "a မှာ [1] ဖြစ်၏။\nb မှာ [2] ဖြစ်၏။\nprint(a < b);\n",
        ),
        ("order_mixed", "print(\"before\");\nprint([1] <= 2);\n"),
        (
            "order_kinds",
            "t မှာ (1, 2) ဖြစ်၏။\ns မှာ {1} ဖြစ်၏။\nprint(t > s);\n",
        ),
        ("store_range", "xs မှာ [1, 2] ဖြစ်၏။\nxs[5] က 9။\n"),
        ("store_negative", "xs မှာ [1, 2] ဖြစ်၏။\nxs[-1] က 9။\n"),
        ("set_store_range", "s မှာ {1, 2} ဖြစ်၏။\ns[5] က 9။\n"),
        // A string index past its clusters: the lowered cluster list makes the
        // runtime check the same bounds and report the same `E049` line.
        ("string_index_range", "print(\"မြန်မာ\"[4]);\n"),
    ];
    let mut failures = Vec::new();
    for (name, program) in cases {
        for optimize in [true, false] {
            let label = if optimize { "opt" } else { "no-opt" };
            let dir = work_dir(name, label);
            let source = dir.join(format!("{name}.akk"));
            fs::write(&source, program).expect("the source");

            let reference = interpret(&source);
            let built = build(&source, optimize);
            if !built.status.success() {
                failures.push(format!(
                    "{name} ({label}): `akk build` failed: {}",
                    text(&built.stderr).trim()
                ));
                continue;
            }
            let native = execute(&executable_for(&source));
            if native.status.code() != reference.status.code() {
                failures.push(format!(
                    "{name} ({label}): exit {:?}, interpreter exited {:?}",
                    native.status.code(),
                    reference.status.code()
                ));
            }
            if program_output(text(&native.stdout)) != program_output(text(&reference.stdout)) {
                failures.push(format!(
                    "{name} ({label}): stdout {:?}, interpreter {:?}",
                    program_output(text(&native.stdout)),
                    program_output(text(&reference.stdout))
                ));
            }
            if text(&native.stderr) != text(&reference.stderr) {
                failures.push(format!(
                    "{name} ({label}): stderr {:?}, interpreter {:?}",
                    text(&native.stderr),
                    text(&reference.stderr)
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} collection diagnostic check(s) failed:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

/// Nested collections: a list of lists, a deeper list, a tuple of tuples and
/// a dict of lists are indexed twice, printed whole, looped, compared and
/// replaced through their outer element. Each program runs on the interpreter
/// and on the built executable in both optimiser modes, and stdout, stderr and
/// the exit code must all agree.
#[test]
fn nested_collections_match_the_interpreter() {
    if !cfg!(target_arch = "x86_64") {
        return;
    }
    let cases = [
        (
            "nested",
            "grid မှာ [[1, 2], [3, 4]] ဖြစ်၏။\n\
             print(grid[0][1]);\n\
             print(grid);\n\
             grid[0] က [9, 8]။\n\
             print(grid[0][0] + grid[1][1]);\n\
             deep မှာ [[[1, 2], [3]], [[4]]] ဖြစ်၏။\n\
             print(deep[0][1][0]);\n\
             print(deep[0][0][1 - 1]);\n\
             print(deep == [[[1, 2], [3]], [[4]]]);\n\
             pairs မှာ ((1, 2), (3, 4)) ဖြစ်၏။\n\
             print(pairs[1][0]);\n\
             d မှာ { \"a\" က [1, 2]။ \"b\" က [3]။ } ဖြစ်၏။\n\
             print(d[\"b\"][0]);\n\
             print(d[\"a\"] == [1, 2]);\n\
             k သည် d ထဲမှ တစ်ခုစီ\n\
                 print(d[k]);\n\
             ပြီး။\n\
             row သည် grid ထဲမှ တစ်ခုစီ\n\
                 cell သည် row ထဲမှ တစ်ခုစီ\n\
                     cell ကို ဖော်ပြပါ။\n\
                 ပြီး။\n\
             ပြီး။\n",
        ),
        // The second level reports the same `E049` line as the interpreter.
        (
            "nested_index_range",
            "grid မှာ [[1, 2]] ဖြစ်၏။\nprint(grid[0][5]);\n",
        ),
        // Ordering a list of lists is the interpreter's `E042` for `list`.
        (
            "nested_order",
            "grid မှာ [[1], [2]] ဖြစ်၏။\nprint(grid < [[3]]);\n",
        ),
        // An empty inner literal leaves the inner shape open; the second
        // element settles it, and reading the empty one is the same `E049`.
        (
            "nested_prefix_shape",
            "xs မှာ [[], [1]] ဖြစ်၏။\nprint(xs[1][0]);\nprint(xs);\nprint(xs[0][0]);\n",
        ),
        // A failed replacement leaves the binding as the interpreter has it:
        // the copy-on-write store only assigns after the bounds check.
        (
            "nested_store_range",
            "grid မှာ [[1]] ဖြစ်၏။\ngrid[0] က [2, 3]။\nprint(grid);\ngrid[4] က [5]။\n",
        ),
    ];
    let mut failures = Vec::new();
    for (name, program) in cases {
        for optimize in [true, false] {
            let label = if optimize { "opt" } else { "no-opt" };
            let dir = work_dir(name, label);
            let source = dir.join(format!("{name}.akk"));
            fs::write(&source, program).expect("the source");

            let reference = interpret(&source);
            let built = build(&source, optimize);
            if !built.status.success() {
                failures.push(format!(
                    "{name} ({label}): `akk build` failed: {}",
                    text(&built.stderr).trim()
                ));
                continue;
            }
            let native = execute(&executable_for(&source));
            if native.status.code() != reference.status.code() {
                failures.push(format!(
                    "{name} ({label}): exit {:?}, interpreter exited {:?}",
                    native.status.code(),
                    reference.status.code()
                ));
            }
            if program_output(text(&native.stdout)) != program_output(text(&reference.stdout)) {
                failures.push(format!(
                    "{name} ({label}): stdout {:?}, interpreter {:?}",
                    program_output(text(&native.stdout)),
                    program_output(text(&reference.stdout))
                ));
            }
            if text(&native.stderr) != text(&reference.stderr) {
                failures.push(format!(
                    "{name} ({label}): stderr {:?}, interpreter {:?}",
                    text(&native.stderr),
                    text(&reference.stderr)
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} nested collection check(s) failed:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

/// The collection arena is fixed at 256 KiB. A single list literal larger than
/// that must take the documented exhaustion path: `E177` on stderr and exit 1,
/// with no memory corruption and no success. The interpreter, which has no such
/// arena, runs the same program normally, so the test also proves the program
/// itself is valid.
#[test]
fn the_collection_arena_reports_exhaustion_honestly() {
    if !cfg!(target_arch = "x86_64") {
        return;
    }
    // `16 + 16 * 16400` bytes for one list descriptor: 262416, past the
    // 262144-byte arena. A single literal keeps the test cheap and makes the
    // failure happen on the constructor's first `rt_alloc`.
    let items = vec!["0"; 16_400].join(", ");
    let program = format!("xs မှာ [{items}] ဖြစ်၏။\nprint(len(xs));\n");
    let mut failures = Vec::new();
    for optimize in [true, false] {
        let label = if optimize { "opt" } else { "no-opt" };
        let dir = work_dir("arena_exhaustion", label);
        let source = dir.join("arena_exhaustion.akk");
        fs::write(&source, &program).expect("the source");

        let reference = interpret(&source);
        if !reference.status.success() || program_output(text(&reference.stdout)) != "16400\n" {
            failures.push(format!(
                "{label}: the interpreter should print 16400 and exit 0, got {:?} / {:?}",
                reference.status.code(),
                text(&reference.stderr).trim()
            ));
            continue;
        }

        let built = build(&source, optimize);
        if !built.status.success() {
            failures.push(format!(
                "{label}: `akk build` failed: {}",
                text(&built.stderr).trim()
            ));
            continue;
        }
        let native = execute(&executable_for(&source));
        let stderr = text(&native.stderr);
        if native.status.code() != Some(1) {
            failures.push(format!(
                "{label}: exhausted program exited {:?}, expected 1",
                native.status.code()
            ));
        }
        if !stderr.starts_with("E177") || !stderr.contains("arena") {
            failures.push(format!(
                "{label}: expected an E177 arena diagnostic, got {stderr:?}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} arena-exhaustion check(s) failed:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}
