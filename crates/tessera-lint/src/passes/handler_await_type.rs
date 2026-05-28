use tessera_ast::*;
use tessera_ast::visitor::Visitor;
use tessera_types::{Type, TypeEnv};
use crate::{Diagnostic, LintPass};

pub struct HandlerAwaitType;

impl LintPass for HandlerAwaitType {
    fn name(&self) -> &'static str { "L-HANDLER-AWAIT-TYPE" }
    fn check(&mut self, program: &Program, env: &TypeEnv) -> Vec<Diagnostic> {
        let mut v = HandlerAwaitVisitor { env, diags: vec![] };
        v.visit_program(program);
        v.diags
    }
}

struct HandlerAwaitVisitor<'e> {
    env: &'e TypeEnv,
    diags: Vec<Diagnostic>,
}

impl<'e> Visitor for HandlerAwaitVisitor<'e> {
    fn visit_expr(&mut self, e: &Expr) {
        if let Expr::MethodCall(m) = e {
            match m.method.name.as_str() {
                "wait" => {
                    if let Expr::Ident(i) = &m.receiver {
                        if let Some(ty) = self.env.lookup(&i.name) {
                            if matches!(ty, Type::HandlerFuture(_)) {
                                self.diags.push(
                                    Diagnostic::error(
                                        "L-HANDLER-AWAIT-TYPE",
                                        "use .waitHandler() or .awaitHandler() on HandlerFuture, not .wait()",
                                        m.span,
                                    )
                                );
                            }
                        }
                    }
                }
                "waitHandler" | "awaitHandler" => {
                    if let Expr::Ident(i) = &m.receiver {
                        if let Some(ty) = self.env.lookup(&i.name) {
                            if matches!(ty, Type::Future(_)) {
                                self.diags.push(
                                    Diagnostic::error(
                                        "L-HANDLER-AWAIT-TYPE",
                                        "use .wait() or await on Future, not .waitHandler()/.awaitHandler()",
                                        m.span,
                                    )
                                );
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        tessera_ast::visitor::walk_expr(self, e);
    }
}
