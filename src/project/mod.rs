//! Everything a project is made of: `akk.toml` (`manifest`), the
//! libraries a program imports (`library`), and the executable `akk build`
//! writes (`bundle`).

pub(crate) mod bundle;
pub(crate) mod library;
pub(crate) mod manifest;
