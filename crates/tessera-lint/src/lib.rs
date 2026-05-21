mod diagnostic;
mod passes;
mod runner;

pub use diagnostic::{Diagnostic, Severity};
pub use runner::{LintRunner, LintPass};
