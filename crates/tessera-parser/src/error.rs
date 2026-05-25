#![allow(dead_code, unused_assignments)]
use miette::{Diagnostic, LabeledSpan};
use tessera_ast::Span;
use thiserror::Error;

#[allow(unused)]
#[derive(Debug, Clone, Error, Diagnostic)]
#[error("{message}")]
#[diagnostic(code(tessera::parse))]
pub struct ParseError {
    pub message: String,
    /// Primary location of the error (kept as a plain field for the public API
    /// and tests; miette rendering uses `labels`).
    pub span: Span,
    /// All labels rendered by miette: the primary span plus any secondary
    /// "opened here" / context spans produced by root-cause analysis.
    #[label(collection)]
    pub labels: Vec<LabeledSpan>,
    #[help]
    pub help: Option<String>,
}

impl ParseError {
    pub fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
            labels: vec![LabeledSpan::new_primary_with_span(None, span)],
            help: None,
        }
    }

    /// Set the text of the primary label (the caret annotation).
    pub fn primary_label(mut self, text: impl Into<String>) -> Self {
        self.labels[0] = LabeledSpan::new_primary_with_span(Some(text.into()), self.span);
        self
    }

    /// Attach an additional, secondary label at a different span (e.g. the
    /// location of an unclosed opening delimiter).
    pub fn with_secondary(mut self, text: impl Into<String>, span: Span) -> Self {
        self.labels.push(LabeledSpan::new_with_span(Some(text.into()), span));
        self
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }
}
