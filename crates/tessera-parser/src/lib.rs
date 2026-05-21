mod error;
mod parser;

pub use error::ParseError;
pub use parser::Parser;

use tessera_ast::Program;
use tessera_lexer::TokenStream;

pub fn parse(tokens: TokenStream) -> (Program, Vec<ParseError>) {
    let mut p = Parser::new(tokens);
    let prog = p.parse_program();
    (prog, p.into_errors())
}
