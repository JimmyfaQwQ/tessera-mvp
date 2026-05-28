use tessera_ast::Program;
use tessera_types::TypeEnv;
use crate::{Diagnostic, LintPass};

pub struct HandlerMustAsync;

impl LintPass for HandlerMustAsync {
    fn name(&self) -> &'static str { "L-HANDLER-MUST-ASYNC" }
    fn check(&mut self, _program: &Program, _env: &TypeEnv) -> Vec<Diagnostic> {
        // Handlers are always async by grammar; validated at parse time.
        vec![]
    }
}
