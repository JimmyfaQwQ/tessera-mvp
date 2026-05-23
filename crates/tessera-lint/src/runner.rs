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
        Self {
            passes: vec![
                Box::new(passes::AwaitAsyncOnly),
                Box::new(passes::HandlerMustAsync),
                Box::new(passes::HandlerAwaitType),
                Box::new(passes::ExposeMutableUnsafe),
                Box::new(passes::GenericTypeArgMissing),
                Box::new(passes::TerminateNonTerminatable),
                Box::new(passes::PermitAwaitInSync),
                Box::new(passes::PermitWaitInAsync),
                Box::new(passes::PermitReleaseNonPositive),
            ],
        }
    }

    pub fn run_all(&mut self, program: &Program, env: &TypeEnv) -> Vec<Diagnostic> {
        let mut all = Vec::new();
        for pass in &mut self.passes {
            all.extend(pass.check(program, env));
        }
        all
    }
}
