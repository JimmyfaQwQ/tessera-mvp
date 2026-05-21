use tessera_ast::Span;
use thiserror::Error;

#[derive(Debug, Clone, Error)]
#[error("{message} at {span:?}")]
pub struct ParseError {
    pub message: String,
    pub span: Span,
}

impl ParseError {
    pub fn new(message: impl Into<String>, span: Span) -> Self {
        Self { message: message.into(), span }
    }
}
