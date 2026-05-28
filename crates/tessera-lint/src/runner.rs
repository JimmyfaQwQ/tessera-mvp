use tessera_ast::Program;
use tessera_types::TypeEnv;
use crate::{Diagnostic, passes};

pub trait LintPass {
    fn name(&self) -> &'static str;
    fn check(&mut self, program: &Program, env: &TypeEnv) -> Vec<Diagnostic>;
}

pub struct LintRunner {
    passes: Vec<Box<dyn LintPass>>,
}

impl LintRunner {
    pub fn default_passes() -> Self {
        Self { passes: passes::all() }
    }

    pub fn run_all(&mut self, program: &Program, env: &TypeEnv) -> Vec<Diagnostic> {
        let mut all = Vec::new();
        for pass in &mut self.passes {
            all.extend(pass.check(program, env));
        }
        all
    }
}
