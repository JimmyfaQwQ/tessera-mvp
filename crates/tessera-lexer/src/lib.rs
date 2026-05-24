#![allow(dead_code, unused_assignments)]
mod token;
pub use token::Token;

use miette::Diagnostic;
use tessera_ast::Span;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct Spanned<T> {
    pub node: T,
    pub span: Span,
}

pub type TokenStream = Vec<Spanned<Token>>;

/// A lexing error: one or more contiguous characters the lexer could not
/// recognise. Previously these were silently turned into `Token::Error`.
#[allow(unused)]
#[derive(Debug, Clone, Error, Diagnostic)]
#[error("unexpected character{plural} `{text}`")]
#[diagnostic(code(tessera::lex::unexpected_char))]
pub struct LexError {
    pub text: String,
    pub plural: &'static str,
    #[label("not a valid Tessera token")]
    pub span: Span,
}

/// Lex the source string and return a flat token stream (whitespace/comments stripped).
/// Any unrecognised characters are turned into `Token::Error`.
pub fn lex(source: &str) -> TokenStream {
    use logos::Logos;
    Token::lexer(source)
        .spanned()
        .map(|(tok, range)| Spanned {
            node: tok.unwrap_or(Token::Error),
            span: Span::new(range.start, range.end),
        })
        .collect()
}

/// Scan a token stream for `Token::Error` tokens and turn them into
/// user-facing diagnostics. Adjacent error tokens are merged into one span so
/// that e.g. a run of invalid bytes produces a single message.
pub fn lex_errors(source: &str, tokens: &TokenStream) -> Vec<LexError> {
    let mut errors = Vec::new();
    let mut pending: Option<Span> = None;

    let flush = |span: Span, errors: &mut Vec<LexError>| {
        let text: String = source.get(span.start..span.end).unwrap_or("").to_string();
        let plural = if text.chars().count() > 1 { "s" } else { "" };
        errors.push(LexError { text, plural, span });
    };

    for tok in tokens {
        if matches!(tok.node, Token::Error) {
            pending = Some(match pending {
                Some(prev) if prev.end == tok.span.start => prev.merge(tok.span),
                Some(prev) => { flush(prev, &mut errors); tok.span }
                None => tok.span,
            });
        } else if let Some(prev) = pending.take() {
            flush(prev, &mut errors);
        }
    }
    if let Some(prev) = pending.take() {
        flush(prev, &mut errors);
    }
    errors
}
