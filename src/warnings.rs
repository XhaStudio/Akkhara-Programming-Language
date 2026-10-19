//! Static warnings -- things a file contains that the program never uses.
//!
//! The pass runs once, over the parsed statement list, after parsing and
//! before the program executes. A warning never changes what a program does:
//! it is printed (Burmese, one line each, on stderr -- and into the run's
//! `.akop` transcript) and the program runs on. Each one carries its own code:
//!
//! | code | meaning | printed as |
//! |---|---|---|
//! | `W001` | a variable that is assigned but never read | unused variable |
//! | `W002` | an imported library that is never used | unused import |
//! | `W003` | a statement after `return` / `break` / `continue` | dead code |
//! | `W004` | a function parameter the body never reads | unused argument |
//! | `W005` | a function the program never calls | unused function |
//! | `W006` | a bare value whose result is thrown away | statement with no effect |
//! | `W007` | a comparison used as a statement | unused comparison |
//!
//! The analysis is deliberately conservative -- anything that *might* be a use
//! counts as one -- so a warning means the file really never uses the thing.
//!
//! Imports are followed: a script or package library the program brings in
//! with `နည်းပညာများ <name> ကို သုံးမည်။` is checked too, and so is a library
//! that library imports. Each file keeps its own variables, functions and
//! imports, but the *uses* are pooled across all of them, because an
//! imported library runs in the program's own global scope: a name a library
//! assigns may be read by the program, and a function a library defines may
//! be called from anywhere. A library's warnings are prefixed with the file
//! they are in (`Tools.akk: W001 ...`); the program's own lines are not.

use std::collections::{HashMap, HashSet};

use crate::parser::{CondChain, Expr, ForSource, IfBranch, Stmt};

/// A script or package library an import resolves to: the file to name in a
/// warning, and its source text.
pub struct Library {
    pub file: String,
    pub source: String,
}

/// The self-field marker inside an interpolation placeholder -- the same
/// spelling the interpreter uses (`{အတု <field>}`).
const SELF_PLACEHOLDER: &str = "အတု";

/// Check a parsed program and every script or package library it imports,
/// following imports through one library into the next. `resolve` turns an
/// imported name into its source and file (`None` for a built-in library,
/// whose code is in the binary, or a name that is not on disk). A library that
/// cannot be read or parsed is skipped: the run itself reports that import as
/// E066/E067, so there is nothing useful to add before it.
pub fn check_program(stmts: &[Stmt], resolve: &dyn Fn(&str) -> Option<Library>) -> Vec<String> {
    // Per-file findings, in the order the files were reached: the program
    // first, then its libraries as their imports come up.
    let mut analyses: Vec<Collector> = Vec::new();
    // Names already resolved, and files already read, so a diamond (two
    // libraries importing the same third one) or an import cycle is walked
    // once instead of forever.
    let mut resolved: HashSet<String> = HashSet::new();
    let mut read_files: HashSet<String> = HashSet::new();
    let mut pending: Vec<String> = Vec::new();

    let mut root = Collector::default();
    root.block(stmts);
    pending.extend(root.imports.iter().map(|(lib, _, _)| lib.clone()));
    analyses.push(root);

    while let Some(name) = pending.pop() {
        if !resolved.insert(name.clone()) {
            continue;
        }
        let Some(library) = resolve(&name) else {
            continue;
        };
        if !read_files.insert(library.file.clone()) {
            continue;
        }
        let Ok(tokens) = crate::lexer::lex(&library.source) else {
            continue;
        };
        let Ok(lib_stmts) = crate::parser::parse(&tokens) else {
            continue;
        };
        let mut c = Collector::in_file(library.file);
        c.block(&lib_stmts);
        pending.extend(c.imports.iter().map(|(lib, _, _)| lib.clone()));
        analyses.push(c);
    }

    // Uses are pooled across every file before anything is reported, so a
    // function of one library that only another library calls is not called
    // unused just because that file was walked later.
    let mut shared = Shared::default();
    for analysis in &analyses {
        analysis.absorb(&mut shared);
    }

    analyses
        .iter()
        .flat_map(|analysis| {
            let file = analysis.file.as_deref();
            analysis
                .finish(&shared)
                .into_iter()
                .map(move |line| match file {
                    Some(file) => format!("{}: {}", file, line),
                    None => line,
                })
        })
        .collect()
}

/// Everything the pooled walk found: the reads, calls and library uses of all
/// the files together.
#[derive(Default)]
struct Shared {
    /// Every name read as a value in any file.
    reads: HashSet<String>,
    /// Every name used in a call position, or handed to an alias statement.
    called: HashSet<String>,
    /// Names that appear in a library position: `<lib> ၏ ...`, `<lib>.<fn>`,
    /// an HTTP call through a library, and so on.
    lib_uses: HashSet<String>,
    /// `<lib> အဖြစ် <alias>` pairs from every file: alias -> the library it
    /// stands for. Library aliases are global at run time, like imports.
    lib_alias: HashMap<String, String>,
}

#[derive(Default)]
struct Collector {
    /// The file these findings belong to: `None` for the program itself, the
    /// path for an imported library (whose lines are prefixed with it).
    file: Option<String>,
    /// Every name read anywhere in the file, as a value.
    reads: HashSet<String>,
    /// The read sets of the function bodies currently being walked, innermost
    /// last. A parameter is used when its own body reads it -- and reads by a
    /// nested function count too, since it can reach the same name at run
    /// time.
    frames: Vec<HashSet<String>>,
    /// Every name the file assigns, with the line to blame if it is never
    /// read. A name can appear more than once (a declaration and a later
    /// reassignment); only the first write is reported.
    writes: Vec<(String, usize)>,
    /// Parameters of every function in the file, so a parameter that is only
    /// ever written is W004's business rather than W001's.
    params: HashSet<String>,
    /// Functions the file defines: (name, line), in source order.
    funcs: Vec<(String, usize)>,
    /// Names used in a call position, or handed to an alias statement.
    called: HashSet<String>,
    /// Imports: (library, alias, line), in source order.
    imports: Vec<(String, Option<String>, usize)>,
    /// `<lib> အဖြစ် <alias>` pairs: alias -> the library it stands for.
    lib_alias: HashMap<String, String>,
    /// Names that appear in a library position: `<lib> ၏ ...`, `<lib>.<fn>`,
    /// an HTTP call through a library, and so on.
    lib_uses: HashSet<String>,
    /// (line, finished message) for everything found during the walk.
    out: Vec<Found>,
}

impl Collector {
    /// A collector for an imported library's source, blamed on `file`.
    fn in_file(file: String) -> Self {
        Collector {
            file: Some(file),
            ..Collector::default()
        }
    }

    /// Fold this file's uses into the pooled set. Only the *uses* travel
    /// between files; what a file declares stays its own, so a warning always
    /// points at the file the unused thing is written in.
    fn absorb(&self, shared: &mut Shared) {
        shared.reads.extend(self.reads.iter().cloned());
        shared.called.extend(self.called.iter().cloned());
        shared.lib_uses.extend(self.lib_uses.iter().cloned());
        for (alias, original) in &self.lib_alias {
            shared.lib_alias.insert(alias.clone(), original.clone());
        }
    }

    fn warn(&mut self, line: usize, code: u16, message: String) {
        self.out.push(Found { line, code, message });
    }

    /// Record a read of `name`: the file reads it, and so does every function
    /// body it is inside of.
    fn read(&mut self, name: &str) {
        self.reads.insert(name.to_string());
        for frame in self.frames.iter_mut() {
            frame.insert(name.to_string());
        }
    }

    /// Record an assignment to `name` (a declaration counts as an assignment).
    fn write(&mut self, name: &str, line: usize) {
        self.writes.push((name.to_string(), line));
    }

    /// Walk a block of statements. A statement after `return` / `break` /
    /// `continue` can never run, so each one is W003.
    fn block(&mut self, stmts: &[Stmt]) {
        self.block_with_tail(stmts, false)
    }

    /// Walk a block, knowing whether its final bare expression is the block's
    /// value rather than a discarded one. That is true for a function body:
    /// the interpreter takes its last `ExprStmt` as the call's return value,
    /// so `x * 2` written as the last line of a function is not a statement
    /// with no effect.
    fn block_with_tail(&mut self, stmts: &[Stmt], tail_is_value: bool) {
        let mut dead = false;
        for (i, s) in stmts.iter().enumerate() {
            if dead {
                if let Some(line) = stmt_line(s) {
                    self.warn(line, 3, w003(line));
                }
            }
            let tail = tail_is_value && i + 1 == stmts.len();
            self.stmt_inner(s, tail);
            if is_terminator(s) {
                dead = true;
            }
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        self.stmt_inner(s, false)
    }

    fn stmt_inner(&mut self, s: &Stmt, tail_is_value: bool) {
        match s {
            Stmt::VarDecl { name, value, line } => {
                self.write(name, *line);
                self.expr(value);
            }
            Stmt::Print { value, .. } => self.expr(value),
            Stmt::InputNoAssign { value, .. } => self.expr(value),
            Stmt::InputAssign { name, value, line } => {
                self.write(name, *line);
                self.expr(value);
            }
            Stmt::RandomDecl {
                name, min, max, line,
            } => {
                self.write(name, *line);
                self.expr(min);
                self.expr(max);
            }
            Stmt::ExprStmt { value, line } => {
                // A value that is computed and thrown away: worth a warning
                // only when computing it cannot do anything else, and only
                // when it is not the value its block ends with (see
                // `block_with_tail`).
                if !tail_is_value && !has_effect(value) {
                    if matches!(value, Expr::Compare(..)) {
                        self.warn(*line, 7, w007(*line));
                    } else {
                        self.warn(*line, 6, w006(*line));
                    }
                }
                self.expr(value);
            }
            Stmt::MathAssign {
                target,
                amount,
                line,
                ..
            } => {
                // `x += 1` writes the name; `xs[0] += 1` writes into a
                // collection, so the target is a read.
                match target {
                    Expr::Ident(name) => self.write(name, *line),
                    other => self.expr(other),
                }
                self.expr(amount);
            }
            Stmt::IndexAssign { target, value, .. } => {
                // Writing into a collection reads the collection and the
                // value; there is no variable that becomes "assigned".
                self.expr(target);
                self.expr(value);
            }
            Stmt::ForLoop {
                var_name,
                source,
                body,
                line,
            } => {
                self.write(var_name, *line);
                self.for_source(source);
                self.block(body);
            }
            Stmt::If { branches } => {
                for IfBranch { cond, body, .. } in branches {
                    if let Some(cond) = cond {
                        self.cond(cond);
                    }
                    self.block(body);
                }
            }
            Stmt::While { cond, body, .. } => {
                self.cond(cond);
                self.block(body);
            }
            Stmt::FuncDef {
                name,
                params,
                body,
                line,
            } => {
                self.funcs.push((name.clone(), *line));
                for p in params {
                    self.params.insert(p.clone());
                }
                self.frames.push(HashSet::new());
                // A function body ends with its value when its last statement
                // is a bare expression -- see `block_with_tail`.
                self.block_with_tail(body, true);
                let used = self.frames.pop().unwrap_or_default();
                for p in params {
                    if !used.contains(p) {
                        self.warn(*line, 4, w004(*line, name, p));
                    }
                }
            }
            Stmt::FuncCall { name, args, .. } => {
                self.call(name, args);
            }
            Stmt::FuncCallAssign {
                name,
                fn_name,
                args,
                line,
            } => {
                self.write(name, *line);
                self.call(fn_name, args);
            }
            Stmt::LibCall {
                lib, fn_name, args, ..
            } => {
                self.lib_uses.insert(lib.clone());
                // `<lib> ၏ <fn>` calls a function the library defines, so the
                // library's own `fn <fn>` is used. Which library (through an
                // alias, say) is not resolved here, so any function of that
                // name counts as called -- the conservative reading.
                self.called.insert(fn_name.clone());
                if let Some(callback) = self.app_callback(lib, fn_name, args) {
                    self.called.insert(callback);
                }
                self.args(args);
            }
            Stmt::LibCallAssign {
                name, lib, fn_name, args, line,
            } => {
                self.write(name, *line);
                self.lib_uses.insert(lib.clone());
                self.called.insert(fn_name.clone());
                if let Some(callback) = self.app_callback(lib, fn_name, args) {
                    self.called.insert(callback);
                }
                self.args(args);
            }
            Stmt::FuncAlias {
                original, aliases, ..
            } => {
                // Either a function alias or a library alias -- the file does
                // not say which, so both readings are recorded and a name is
                // treated as used either way.
                self.called.insert(original.clone());
                for alias in aliases {
                    self.called.insert(alias.clone());
                    self.lib_alias.insert(alias.clone(), original.clone());
                }
            }
            Stmt::ClassDef { params, body, .. } => {
                if let Some(params) = params {
                    for p in params {
                        self.params.insert(p.clone());
                    }
                }
                self.block(body);
            }
            Stmt::SelfFieldSet { value, .. } => self.expr(value),
            Stmt::TryCatch {
                try_body,
                catch_body,
                finally_body,
                ..
            } => {
                self.block(try_body);
                if let Some(body) = catch_body {
                    self.block(body);
                }
                if let Some(body) = finally_body {
                    self.block(body);
                }
            }
            Stmt::UseLibrary { libs, line } => {
                for (lib, alias) in libs {
                    self.imports
                        .push((lib.clone(), alias.clone(), *line));
                }
            }
            Stmt::Wait { amount, .. } => self.expr(amount),
            Stmt::HttpSend {
                name, lib, url, data, line,
            } => {
                self.write(name, *line);
                self.lib_uses.insert(lib.clone());
                self.expr(url);
                self.expr(data);
            }
            Stmt::EngDecl { name, value, line, .. } => {
                self.write(name, *line);
                self.expr(value);
            }
            Stmt::EngPrint { value, .. } => self.expr(value),
            Stmt::EngInput { prompt, .. } => self.expr(prompt),
            Stmt::EngInputAssign { name, prompt, line, .. } => {
                self.write(name, *line);
                self.expr(prompt);
            }
            Stmt::EngReturn { value, .. } => {
                if let Some(value) = value {
                    self.expr(value);
                }
            }
            Stmt::EngExprCall { name, args, .. } => {
                self.call(name, args);
            }
            Stmt::EngAssign { name, value, line } => {
                self.write(name, *line);
                self.expr(value);
            }
            Stmt::EngMathAssign { name, amount, line, .. } => {
                self.write(name, *line);
                self.expr(amount);
            }
            Stmt::EngBreak { value, .. } => {
                if let Some(value) = value {
                    self.expr(value);
                }
            }
            Stmt::Continue { .. } => {}
            Stmt::EngLoop { body } => self.block(body),
            Stmt::EngFor {
                init,
                cond,
                step,
                body,
                ..
            } => {
                self.stmt(init);
                self.cond(cond);
                self.stmt(step);
                self.block(body);
            }
            Stmt::EngLoopAssign { name, body, line, .. } => {
                self.write(name, *line);
                self.block(body);
            }
        }
    }

    fn for_source(&mut self, source: &ForSource) {
        match source {
            ForSource::Range {
                start,
                end,
                step,
                ..
            } => {
                if let Some(start) = start {
                    self.expr(start);
                }
                self.expr(end);
                self.expr(step);
            }
            ForSource::RangeSigned { start, end, step } => {
                self.expr(start);
                self.expr(end);
                self.expr(step);
            }
            ForSource::Auto(value) => self.expr(value),
        }
    }

    fn cond(&mut self, chain: &CondChain) {
        let atoms = std::iter::once(&chain.first).chain(chain.rest.iter().map(|(_, a)| a));
        for atom in atoms {
            self.expr(&atom.lhs);
            if let Some(rhs) = &atom.rhs {
                self.expr(rhs);
            }
        }
    }

    fn args(&mut self, args: &[Expr]) {
        for a in args {
            self.expr(a);
        }
    }

    /// Names a string literal interpolates. Every string literal is
    /// interpolated when it is evaluated, so `{name}` is a read of that
    /// variable -- a value only ever used inside a string (`"Hello, {who}!"`)
    /// is not unused. The interpreter's spelling is followed here too: `\{`
    /// and `\}` are literal braces, and a `{အတု <field>}` placeholder reads a
    /// field off the object under construction rather than a variable.
    fn read_interpolated(&mut self, s: &str) {
        if !s.contains('{') {
            return;
        }
        let mut rest = s;
        while let Some(start) = rest.find('{') {
            // An escaped brace is text, not a placeholder.
            if start > 0 && rest.as_bytes()[start - 1] == b'\\' {
                rest = &rest[start + 1..];
                continue;
            }
            let Some(end) = rest[start..].find('}') else {
                return; // An unclosed placeholder is E056 when it runs.
            };
            let name = rest[start + 1..start + end].trim();
            if !name.is_empty() && !name.starts_with(SELF_PLACEHOLDER) {
                self.read(name);
            }
            rest = &rest[start + end + 1..];
        }
    }

    /// The Akkhara function an `App` call registers as a callback, if any.
    ///
    /// `App.button(w, "Save", 20, 60, "on_save")`, `App.on_key(w, "Enter", ...)`
    /// and `App.every(w, 1000, ...)` take the callback as *text*, which the
    /// library calls by name later on -- so the name is a use of that
    /// function even though no call site spells it out. Without this the walk
    /// would report the handler as an unused function (W005).
    fn app_callback(&self, lib: &str, fn_name: &str, args: &[Expr]) -> Option<String> {
        let is_app = lib == "App"
            || self
                .lib_alias
                .get(lib)
                .map(|original| original == "App")
                .unwrap_or(false);
        if !is_app {
            return None;
        }
        let index = match fn_name {
            "button" => 4,
            "on_key" | "every" => 2,
            _ => return None,
        };
        match args.get(index) {
            Some(Expr::StrLit(name)) => Some(name.clone()),
            _ => None,
        }
    }

    /// Record a call: its callee name, and -- for the higher-order builtins --
    /// the bare function name passed as the callback, which is called too.
    fn call(&mut self, name: &str, args: &[Expr]) {
        self.called.insert(name.to_string());
        // A call can also go through a variable holding a callable
        // (`f = file.read; f(path)`), so the callee counts as read too.
        self.read(name);
        if matches!(name, "map" | "filter" | "reduce") {
            if let Some(Expr::Ident(callback)) = args.get(1) {
                self.called.insert(callback.clone());
            }
        }
        self.args(args);
    }

    fn expr(&mut self, e: &Expr) {
        match e {
            Expr::StrLit(s) => self.read_interpolated(s),
            Expr::NumLit(_) | Expr::Int(_) | Expr::Float(_) | Expr::BoolLit(_) | Expr::NullLit => {}
            Expr::Ident(name) => self.read(name),
            Expr::Binary(a, _, b, _) | Expr::Compare(a, _, b, _) => {
                self.expr(a);
                self.expr(b);
            }
            Expr::Neg(a, _) => self.expr(a),
            Expr::ListLit(items) | Expr::TupleLit(items) | Expr::SetLit(items) => {
                for item in items {
                    self.expr(item);
                }
            }
            Expr::DictLit(pairs) => {
                for (k, v) in pairs {
                    self.expr(k);
                    self.expr(v);
                }
            }
            Expr::Index(base, indices) => {
                self.expr(base);
                for index in indices {
                    self.expr(index);
                }
            }
            Expr::NewObj(name, args, _) => {
                // `name(args)` in expression position: a call to a user
                // function or a class constructor, so it counts as a use of
                // that name either way.
                self.call(name, args);
            }
            Expr::LibCall(lib, fn_name, args, _) => {
                self.lib_uses.insert(lib.clone());
                self.called.insert(fn_name.clone());
                if let Some(callback) = self.app_callback(lib, fn_name, args) {
                    self.called.insert(callback);
                }
                self.args(args);
            }
            Expr::LibFnRef(lib, _, _) => {
                self.lib_uses.insert(lib.clone());
            }
            Expr::FieldAccess(base, _, _) => self.expr(base),
            Expr::HttpGet(lib, url, _) => {
                self.lib_uses.insert(lib.clone());
                self.expr(url);
            }
        }
    }

    /// Turn what the walk collected into the finished warning lines for this
    /// file, in source order. `shared` decides whether a name is used
    /// somewhere in the program as a whole.
    fn finish(&self, shared: &Shared) -> Vec<String> {
        // An import is used when its own name -- or an alias of it -- appears
        // in a library position. Aliases chain: `r` standing for `ကျပန်း`
        // makes `ကျပန်း ၏ ...` the used one.
        let mut used = shared.lib_uses.clone();
        loop {
            let before = used.len();
            for (alias, original) in &shared.lib_alias {
                if used.contains(alias) {
                    used.insert(original.clone());
                }
            }
            if used.len() == before {
                break;
            }
        }

        let mut found: Vec<Found> = Vec::new();
        let mut seen: HashSet<&str> = HashSet::new();
        for (name, line) in &self.writes {
            if self.params.contains(name) || shared.reads.contains(name) {
                continue;
            }
            if seen.insert(name.as_str()) {
                found.push(Found {
                    line: *line,
                    code: 1,
                    message: w001(*line, name),
                });
            }
        }
        for (lib, alias, line) in &self.imports {
            let used_lib = used.contains(lib)
                || alias.as_ref().is_some_and(|a| used.contains(a))
                // `file` and `ဖိုင်` are one library, so using one spelling
                // counts as using the other (same for json/server).
                || library_sibling(lib).is_some_and(|other| used.contains(other));
            if !used_lib {
                found.push(Found {
                    line: *line,
                    code: 2,
                    message: w002(*line, lib),
                });
            }
        }
        let mut seen_funcs: HashSet<&str> = HashSet::new();
        for (name, line) in &self.funcs {
            if !shared.called.contains(name) && seen_funcs.insert(name.as_str()) {
                found.push(Found {
                    line: *line,
                    code: 5,
                    message: w005(*line, name),
                });
            }
        }

        found.extend(self.out.iter().cloned());
        // Source order, and by code when two warnings share a line, so the
        // report of a file always comes out the same way.
        found.sort_by_key(|f| (f.line, f.code));
        found.into_iter().map(|f| f.message).collect()
    }
}

/// One warning found: where it points, its code (1 for `W001`, ...) and the
/// finished line to print.
#[derive(Clone)]
struct Found {
    line: usize,
    code: u16,
    message: String,
}

/// Whether evaluating `e` could do anything besides produce a value. Only a
/// library call, an object construction and an HTTP fetch are assumed to have
/// an effect; everything else is arithmetic over values.
fn has_effect(e: &Expr) -> bool {
    match e {
        Expr::LibCall(..) | Expr::NewObj(..) | Expr::HttpGet(..) => true,
        Expr::Binary(a, _, b, _) | Expr::Compare(a, _, b, _) => has_effect(a) || has_effect(b),
        Expr::Neg(a, _) => has_effect(a),
        Expr::ListLit(items) | Expr::TupleLit(items) | Expr::SetLit(items) => {
            items.iter().any(has_effect)
        }
        Expr::DictLit(pairs) => pairs.iter().any(|(k, v)| has_effect(k) || has_effect(v)),
        Expr::Index(base, indices) => {
            has_effect(base) || indices.iter().any(has_effect)
        }
        Expr::FieldAccess(base, _, _) => has_effect(base),
        _ => false,
    }
}

/// A statement that ends the run of its block: nothing after it can execute.
fn is_terminator(s: &Stmt) -> bool {
    matches!(
        s,
        Stmt::EngReturn { .. } | Stmt::EngBreak { .. } | Stmt::Continue { .. }
    )
}

/// The line a statement is written on, for the warnings that point at one.
/// The few statements the parser builds without a line of their own (a class
/// body, an infinite `loop`) report the first line they contain, or nothing.
fn stmt_line(s: &Stmt) -> Option<usize> {
    match s {
        Stmt::VarDecl { line, .. }
        | Stmt::Print { line, .. }
        | Stmt::InputNoAssign { line, .. }
        | Stmt::InputAssign { line, .. }
        | Stmt::RandomDecl { line, .. }
        | Stmt::ExprStmt { line, .. }
        | Stmt::MathAssign { line, .. }
        | Stmt::IndexAssign { line, .. }
        | Stmt::ForLoop { line, .. }
        | Stmt::While { line, .. }
        | Stmt::FuncDef { line, .. }
        | Stmt::FuncCall { line, .. }
        | Stmt::FuncCallAssign { line, .. }
        | Stmt::LibCall { line, .. }
        | Stmt::LibCallAssign { line, .. }
        | Stmt::FuncAlias { line, .. }
        | Stmt::SelfFieldSet { line, .. }
        | Stmt::UseLibrary { line, .. }
        | Stmt::Wait { line, .. }
        | Stmt::HttpSend { line, .. }
        | Stmt::EngDecl { line, .. }
        | Stmt::EngPrint { line, .. }
        | Stmt::EngInput { line, .. }
        | Stmt::EngInputAssign { line, .. }
        | Stmt::EngReturn { line, .. }
        | Stmt::EngExprCall { line, .. }
        | Stmt::EngAssign { line, .. }
        | Stmt::EngMathAssign { line, .. }
        | Stmt::EngBreak { line, .. }
        | Stmt::Continue { line }
        | Stmt::EngFor { line, .. }
        | Stmt::EngLoopAssign { line, .. }
        | Stmt::ClassDef { line, .. } => Some(*line),
        Stmt::If { branches } => branches
            .iter()
            .filter_map(|b| b.cond.as_ref())
            .map(|c| c.first.line)
            .min(),
        Stmt::TryCatch { try_body, .. } => try_body.first().and_then(stmt_line),
        Stmt::EngLoop { body } => body.first().and_then(stmt_line),
    }
}

fn w001(line: usize, name: &str) -> String {
    format!(
        "W001 လိုင်း {} ၏ variable \"{}\" ကို အသုံးမပြုထားပါ (unused variable)။",
        line, name
    )
}

/// The other spelling of a library that answers to two names, so an import
/// and a use written in different languages are not reported as unused.
fn library_sibling(name: &str) -> Option<&'static str> {
    match name {
        "file" => Some("ဖိုင်"),
        "ဖိုင်" => Some("file"),
        "json" => Some("ဂျေဆန်"),
        "ဂျေဆန်" => Some("json"),
        "server" => Some("ဆာဗာ"),
        "ဆာဗာ" => Some("server"),
        _ => None,
    }
}

fn w002(line: usize, lib: &str) -> String {
    format!(
        "W002 လိုင်း {} ၏ အသုံးပြုရန် ခေါ်ထားသော \"{}\" library ကို အသုံးမပြုထားပါ (unused import)။",
        line, lib
    )
}

fn w003(line: usize) -> String {
    format!(
        "W003 လိုင်း {} ၏ statement ကို ရောက်ရှိမည် မဟုတ်ပါ (dead code)။",
        line
    )
}

fn w004(line: usize, func: &str, param: &str) -> String {
    format!(
        "W004 လိုင်း {} ၏ function \"{}\" argument \"{}\" ကို အသုံးမပြုထားပါ (unused argument)။",
        line, func, param
    )
}

fn w005(line: usize, name: &str) -> String {
    format!(
        "W005 လိုင်း {} ၏ function \"{}\" ကို ခေါ်သုံးမထားပါ (unused function)။",
        line, name
    )
}

fn w006(line: usize) -> String {
    format!(
        "W006 လိုင်း {} ၏ statement သည် ဘာအကျိုးသက်ရောက်မှုမှ မရှိပါ (statement with no effect)။",
        line
    )
}

fn w007(line: usize) -> String {
    format!(
        "W007 လိုင်း {} ၏ နှိုင်းယှဉ်ချက် ရလဒ်ကို အသုံးမပြုထားပါ (unused comparison)။",
        line
    )
}
