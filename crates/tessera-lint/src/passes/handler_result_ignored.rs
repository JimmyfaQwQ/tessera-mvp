//! L-HANDLER-RESULT-IGNORED: a bare handler dispatch (`h.foo();`) drops the
//! returned `HandlerFuture` immediately — the future is dropped before it
//! can even surface a dispatch error, so the caller has no way to learn
//! whether the target was alive. This is almost always a bug; warn when an
//! `ExprStmt` is `<thread_handle>.<handler>(...)` for any handler other
//! than the privileged `terminate` / `__ping__`.
//!
//! Limitation: like `handler_await_type`, we only resolve `Expr::Ident`
//! receivers via `TypeEnv::lookup`. More complex receivers (field chains,
//! call returns) are conservatively passed over.

use tessera_ast::*;
use tessera_ast::visitor::Visitor;
use tessera_types::{Type, TypeEnv};
use crate::{Diagnostic, LintPass};
use super::helpers::infer_expr_type;

pub struct HandlerResultIgnored;

impl LintPass for HandlerResultIgnored {
    fn name(&self) -> &'static str { "L-HANDLER-RESULT-IGNORED" }
    fn check(&mut self, program: &Program, env: &TypeEnv) -> Vec<Diagnostic> {
        let mut v = Visitor_ { env, diags: vec![] };
        v.visit_program(program);
        v.diags
    }
}

struct Visitor_<'e> {
    env: &'e TypeEnv,
    diags: Vec<Diagnostic>,
}

impl<'e> Visitor for Visitor_<'e> {
    fn visit_stmt(&mut self, s: &Stmt) {
        if let Stmt::Expr(es) = s {
            if let Expr::MethodCall(m) = &es.expr {
                // The lightweight typer covers field-chain receivers (e.g.
                // `self.worker.echo(...)`), so the lint catches the common
                // dispatch-and-forget pattern even when the handle is reached
                // through a field rather than a bare local.
                if let Some(Type::ThreadHandle(id)) = infer_expr_type(self.env, &m.receiver) {
                    let method = m.method.name.as_str();
                    // Privileged methods that never need result inspection.
                    if method != "terminate" && method != "__ping__" {
                        let is_handler = self.env.templates.values()
                            .find(|(tid, _)| *tid == id)
                            .map(|(_, info)| info.handlers.contains_key(method))
                            .unwrap_or(false);
                        if is_handler {
                            self.diags.push(
                                Diagnostic::warn(
                                    "L-HANDLER-RESULT-IGNORED",
                                    format!("the HandlerFuture returned by `{method}` is dropped without checking dispatch success"),
                                    m.span,
                                )
                                .with_help("bind the result and inspect it via `.isErr()` or `try await ...` to detect target-thread failures"),
                            );
                        }
                    }
                }
            }
        }
        tessera_ast::visitor::walk_stmt(self, s);
    }
}
