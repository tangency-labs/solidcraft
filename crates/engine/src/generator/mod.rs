//! Generators: a manufacturer's ordering code to a parametric part (docs/formats/generator.md,
//! RFC 0017, #22331).
//!
//! A generator is a parametric design modelled once, a configuration table of catalogue rows
//! and an ordering-code template. This module reads a package folder, reads a code with its
//! field templates, runs its sandboxed Rhai script to turn the code's fields into a row and
//! parameter values, and hands those back as plain data. The commands in `cmd/generator.rs`
//! insert the design with them. Nothing here runs a third-party program, reaches the network
//! or panics; every bound is a typed error.

pub mod code_template;
pub mod csv_table;
pub mod installed;
pub mod package;
pub mod script;

#[cfg(test)]
mod code_template_tests;
#[cfg(test)]
mod script_tests;
#[cfg(test)]
pub(crate) mod tests;

pub use package::{Package, Refusal, Resolved, RowInfo};
pub use script::RefusalKind;

/// Why a package did not load.
#[derive(Clone, Debug, PartialEq)]
pub enum GeneratorError {
    /// The package is malformed.
    Package(String),
    /// The package needs a newer SolidCraft.
    NeedsNewer { needs: String, have: String },
    /// A bound was exceeded.
    Limit { what: &'static str, limit: usize },
    /// The package's script did not compile or run.
    Script(script::ScriptError),
}

impl std::fmt::Display for GeneratorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GeneratorError::Package(m) => write!(f, "generator package: {m}"),
            GeneratorError::NeedsNewer { needs, have } => write!(f, "this generator needs SolidCraft {needs} or newer (this is {have})"),
            GeneratorError::Limit { what, limit } => write!(f, "generator limit: more than {limit} {what}"),
            GeneratorError::Script(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for GeneratorError {}
