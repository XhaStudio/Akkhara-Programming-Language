//! The static check every run performs before the program executes: the
//! program itself, plus every script or package it imports.

use std::path::PathBuf;

use crate::frontend::parser::Stmt;
use crate::project::library::LibraryLoader;
use crate::sema::warnings;

/// Check a program and the script/package libraries it imports, resolving an
/// import the way a run does (a downloaded package first, then a plain
/// `<name>.akk` script in one of `search_dirs`). A built-in library and a name
/// that is not on disk add nothing: there is no source to read.
///
/// `libraries_dir` is the folder `akk install` downloads packages into, next
/// to the binary.
pub(crate) fn library_warnings(
    libraries_dir: PathBuf,
    stmts: &[Stmt],
    search_dirs: &[PathBuf],
    dependencies: &[(String, PathBuf)],
) -> Vec<String> {
    let mut loader = LibraryLoader::new(libraries_dir);
    for dir in search_dirs {
        loader.add_script_dir(dir.clone());
    }
    for (name, path) in dependencies {
        loader.add_dependency(name, path.clone());
    }
    warnings::check_program(stmts, &|name| {
        loader
            .find_source(name)
            .map(|(path, source)| warnings::Library {
                file: path.display().to_string(),
                source,
            })
    })
}
