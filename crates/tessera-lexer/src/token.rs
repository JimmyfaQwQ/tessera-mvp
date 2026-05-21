use logos::Logos;

#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(skip r"[ \t\r\n]+")]
#[logos(skip r"//[^\n]*")]
#[logos(skip r"/\*([^*]|\*[^/])*\*/")]
pub enum Token {
    // ── Compound sigil keywords (must come before bare $ / @ / #) ────────────
    #[token("$template")]
    KwDollarTemplate,

    #[token("@template")]
    KwAtTemplate,

    /// Anonymous thread shorthand: `${ ... }`
    #[token("${")]
    DollarBraceOpen,

    /// Exclusive block: `#exclusive { ... }`
    #[token("#exclusive")]
    KwExclusive,

    // ── Post-bind operator `:=` (before bare `:`) ─────────────────────────────
    #[token(":=")]
    ColonEq,

    // ── Declaration keywords ──────────────────────────────────────────────────
    #[token("function")]
    KwFunction,

    #[token("async")]
    KwAsync,

    #[token("handler")]
    KwHandler,

    #[token("expose_mutable")]
    KwExposeMutable,

    #[token("expose")]
    KwExpose,

    #[token("let")]
    KwLet,

    // ── Control flow ──────────────────────────────────────────────────────────
    #[token("if")]
    KwIf,

    #[token("else")]
    KwElse,

    #[token("while")]
    KwWhile,

    #[token("for")]
    KwFor,

    #[token("break")]
    KwBreak,

    #[token("continue")]
    KwContinue,

    #[token("return")]
    KwReturn,

    #[token("await")]
    KwAwait,

    // ── Error / assertion ─────────────────────────────────────────────────────
    #[token("panic")]
    KwPanic,

    #[token("assert")]
    KwAssert,

    // ── Type keywords ─────────────────────────────────────────────────────────
    #[token("bool")]
    KwBool,

    #[token("int")]
    KwInt,

    #[token("double")]
    KwDouble,

    #[token("char")]
    KwChar,

    #[token("String")]
    KwString,

    #[token("void")]
    KwVoid,

    #[token("never")]
    KwNever,

    // ── Literals ──────────────────────────────────────────────────────────────
    #[token("true")]
    LitTrue,

    #[token("false")]
    LitFalse,

    #[regex(r"[0-9]+\.[0-9]+([eE][+-]?[0-9]+)?", lex_double)]
    LitDouble(f64),

    #[regex(r"[0-9]+", lex_int)]
    LitInt(i64),

    #[regex(r#""([^"\\]|\\.)*""#, lex_string)]
    LitString(String),

    #[regex(r"'([^'\\]|\\.)'", lex_char)]
    LitChar(char),

    // ── Identifiers (after all keywords to avoid masking) ────────────────────
    #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice().to_string())]
    Ident(String),

    // ── Sigils (bare, after compound forms) ───────────────────────────────────
    #[token("$")]
    Dollar,

    #[token("@")]
    At,

    // ── Operators (two-char before one-char) ─────────────────────────────────
    #[token("==")]
    EqEq,

    #[token("!=")]
    BangEq,

    #[token("<=")]
    LtEq,

    #[token(">=")]
    GtEq,

    #[token("&&")]
    AmpAmp,

    #[token("||")]
    PipePipe,

    #[token("=")]
    Eq,

    #[token("+")]
    Plus,

    #[token("-")]
    Minus,

    #[token("*")]
    Star,

    #[token("/")]
    Slash,

    #[token("%")]
    Percent,

    #[token("!")]
    Bang,

    #[token("<")]
    Lt,

    #[token(">")]
    Gt,

    // ── Punctuation ───────────────────────────────────────────────────────────
    #[token("{")]
    BraceOpen,

    #[token("}")]
    BraceClose,

    #[token("(")]
    ParenOpen,

    #[token(")")]
    ParenClose,

    #[token("[")]
    BracketOpen,

    #[token("]")]
    BracketClose,

    #[token(";")]
    Semicolon,

    #[token(",")]
    Comma,

    #[token(":")]
    Colon,

    #[token(".")]
    Dot,

    // ── Error recovery ────────────────────────────────────────────────────────
    Error,
}

fn lex_int(lex: &mut logos::Lexer<Token>) -> Option<i64> {
    lex.slice().parse().ok()
}

fn lex_double(lex: &mut logos::Lexer<Token>) -> Option<f64> {
    lex.slice().parse().ok()
}

fn lex_string(lex: &mut logos::Lexer<Token>) -> Option<String> {
    let raw = lex.slice();
    let inner = &raw[1..raw.len() - 1];
    Some(unescape(inner))
}

fn lex_char(lex: &mut logos::Lexer<Token>) -> Option<char> {
    let raw = lex.slice();
    let inner = &raw[1..raw.len() - 1];
    unescape(inner).chars().next()
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('"') => out.push('"'),
                Some('\'') => out.push('\''),
                Some('\\') => out.push('\\'),
                Some('0') => out.push('\0'),
                Some(other) => { out.push('\\'); out.push(other); }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use logos::Logos;

    fn single(src: &str) -> Token {
        let mut lex = Token::lexer(src);
        lex.next().unwrap().unwrap()
    }

    #[test]
    fn keywords() {
        assert_eq!(single("$template"), Token::KwDollarTemplate);
        assert_eq!(single("@template"), Token::KwAtTemplate);
        assert_eq!(single("${"), Token::DollarBraceOpen);
        assert_eq!(single("#exclusive"), Token::KwExclusive);
        assert_eq!(single(":="), Token::ColonEq);
        assert_eq!(single("expose_mutable"), Token::KwExposeMutable);
        assert_eq!(single("expose"), Token::KwExpose);
        assert_eq!(single("handler"), Token::KwHandler);
        assert_eq!(single("async"), Token::KwAsync);
    }

    #[test]
    fn dollar_disambiguation() {
        // bare $ after $template and ${ are lexed correctly
        let toks: Vec<_> = Token::lexer("$template $ ${").map(|t| t.unwrap()).collect();
        assert_eq!(toks[0], Token::KwDollarTemplate);
        assert_eq!(toks[1], Token::Dollar);
        assert_eq!(toks[2], Token::DollarBraceOpen);
    }

    #[test]
    fn literals() {
        assert_eq!(single("42"), Token::LitInt(42));
        assert_eq!(single("3.14"), Token::LitDouble(3.14));
        assert_eq!(single("true"), Token::LitTrue);
        assert_eq!(single("false"), Token::LitFalse);
        assert_eq!(single(r#""hello""#), Token::LitString("hello".into()));
        assert_eq!(single("'x'"), Token::LitChar('x'));
    }

    #[test]
    fn two_char_ops() {
        assert_eq!(single(":="), Token::ColonEq);
        assert_eq!(single("=="), Token::EqEq);
        assert_eq!(single("!="), Token::BangEq);
        assert_eq!(single("<="), Token::LtEq);
        assert_eq!(single(">="), Token::GtEq);
        assert_eq!(single("&&"), Token::AmpAmp);
        assert_eq!(single("||"), Token::PipePipe);
    }

    #[test]
    fn comments_stripped() {
        let toks: Vec<_> = Token::lexer("// comment\n42").map(|t| t.unwrap()).collect();
        assert_eq!(toks, vec![Token::LitInt(42)]);
    }
}
