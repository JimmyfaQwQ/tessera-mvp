mod token;
pub use token::Token;

use tessera_ast::Span;

#[derive(Debug, Clone)]
pub struct Spanned<T> {
    pub node: T,
    pub span: Span,
}

pub type TokenStream = Vec<Spanned<Token>>;

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
