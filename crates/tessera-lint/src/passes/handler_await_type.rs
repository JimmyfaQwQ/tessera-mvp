use tessera_ast::*;
use tessera_ast::visitor::Visitor;
use tessera_types::{Type, TypeEnv};
use crate::{Diagnostic, LintPass};
use super::helpers::infer_expr_type;

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
            // Use the lightweight typer so `obj.field.wait()` and
            // `Ok(x).wait()` style receivers are covered, not just bare
            // identifiers.
            let recv_ty = infer_expr_type(self.env, &m.receiver);
            match m.method.name.as_str() {
                "wait" => {
                    if matches!(recv_ty, Some(Type::HandlerFuture(_))) {
                        self.diags.push(
                            Diagnostic::error(
                                "L-HANDLER-AWAIT-TYPE",
                                "use .waitHandler() or .awaitHandler() on HandlerFuture, not .wait()",
                                m.span,
                            )
                        );
                    }
                }
                "waitHandler" | "awaitHandler" => {
                    if matches!(recv_ty, Some(Type::Future(_))) {
                        self.diags.push(
                            Diagnostic::error(
                                "L-HANDLER-AWAIT-TYPE",
                                "use .wait() or await on Future, not .waitHandler()/.awaitHandler()",
                                m.span,
                            )
                        );
                    }
                }
                _ => {}
            }
        }
        tessera_ast::visitor::walk_expr(self, e);
    }
}
