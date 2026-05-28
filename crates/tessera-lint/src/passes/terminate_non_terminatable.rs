use tessera_ast::*;
use tessera_ast::visitor::Visitor;
use tessera_types::{Type, TypeEnv};
use crate::{Diagnostic, LintPass};

pub struct TerminateNonTerminatable;

impl LintPass for TerminateNonTerminatable {
    fn name(&self) -> &'static str { "L-TERMINATE-NON-TERMINATABLE" }
    fn check(&mut self, program: &Program, env: &TypeEnv) -> Vec<Diagnostic> {
        let mut v = TerminateVisitor { env, diags: vec![] };
        v.visit_program(program);
        v.diags
    }
}

struct TerminateVisitor<'e> {
    env: &'e TypeEnv,
    diags: Vec<Diagnostic>,
}

impl<'e> Visitor for TerminateVisitor<'e> {
    fn visit_expr(&mut self, e: &Expr) {
        if let Expr::MethodCall(m) = e {
            if m.method.name == "terminate" {
                if let Expr::Ident(i) = &m.receiver {
                    if let Some(Type::ThreadHandle(id)) = self.env.lookup(&i.name) {
                        for (_, (tid, info)) in &self.env.templates {
                            if tid == id && !info.is_terminatable {
                                self.diags.push(
                                    Diagnostic::error(
                                        "L-TERMINATE-NON-TERMINATABLE",
                                        format!("thread '{}' is not terminatable (no __on_terminate__ defined)", i.name),
                                        m.span,
                                    ).with_help("add 'async function __on_terminate__(): void { ... }' to the thread template")
                                );
                            }
                        }
                    }
                }
            }
        }
        tessera_ast::visitor::walk_expr(self, e);
    }
}
