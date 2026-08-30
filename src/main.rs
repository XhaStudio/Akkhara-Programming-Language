mod interpreter;
mod lexer;
mod parser;

use std::env;
use std::fs;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: akk <file name>");
        process::exit(1);
    }

    let filename = &args[1];
    let src = match fs::read_to_string(filename) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("ဖိုင် \"{}\" ကို ဖတ်၍မရပါ - {}", filename, e);
            process::exit(1);
        }
    };

    let tokens = match lexer::lex(&src) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{}", e);
            process::exit(1);
        }
    };

    let stmts = match parser::parse(&tokens) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{}", e);
            process::exit(1);
        }
    };

    let mut interp = interpreter::Interpreter::new();
    if let Err(e) = interp.run(&stmts) {
        eprintln!("{}", e);
        process::exit(1);
    }
}
