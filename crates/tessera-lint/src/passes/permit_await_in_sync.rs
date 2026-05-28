use tessera_ast::*;
use tessera_ast::visitor::Visitor;
use tessera_types::{Type, TypeEnv};
use crate::{Diagnostic, LintPass};

/// Error: `.awaitPermit()` or `await permitExpr` called outside an async function.
pub struct PermitAwaitInSync;

impl LintPass for PermitAwaitInSync {
    fn name(&self) -> &'static str { "L-PERMIT-AWAIT-IN-SYNC" }
    fn check(&mut self, program: &Program, env: &TypeEnv) -> Vec<Diagnostic> {
        let mut v = PermitAwaitInSyncVisitor { env, in_async: false, diags: vec![] };
        v.visit_program(program);
        v.diags
    }
}

struct PermitAwaitInSyncVisitor<'e> {
    env: &'e TypeEnv,
    in_async: bool,
    diags: Vec<Diagnostic>,
}

impl<'e> Visitor for PermitAwaitInSyncVisitor<'e> {
    fn visit_func_def(&mut self, f: &FuncDef) {
        let old = self.in_async;
        self.in_async = f.kind == FuncKind::Async;
        self.visit_block(&f.body);
        self.in_async = old;
    }

    fn visit_handler_def(&mut self, h: &HandlerDef) {
        let old = self.in_async;
        self.in_async = true;
        self.visit_block(&h.body);
        self.in_async = old;
    }

    fn visit_stmt(&mut self, s: &Stmt) {
        if let Stmt::ThreadSpawn(ts) = s {
            for arg in &ts.args { self.visit_expr(arg); }
            let old = self.in_async;
            self.in_async = true;
            self.visit_block(&ts.body);
            self.in_async = old;
            if let ThreadTemplateRef::Anonymous(decl) = &ts.template {
                self.visit_thread_template_decl(decl);
            }
        } else {
            tessera_ast::visitor::walk_stmt(self, s);
        }
    }

    fn visit_expr(&mut self, e: &Expr) {
        if !self.in_async {
            if let Expr::MethodCall(m) = e {
                if m.method.name == "awaitPermit" {
                    if let Expr::Ident(i) = &m.receiver {
                        if self.env.lookup(&i.name) == Some(&Type::Permit) {
                            self.diags.push(
                                Diagnostic::error(
                                    "L-PERMIT-AWAIT-IN-SYNC",
                                    ".awaitPermit() called in sync context; use .wait() instead",
                                    m.span,
                                )
                            );
                        }
                    }
                }
            }
            // `await permitExpr` in sync context is caught by L-AWAIT-ASYNC-ONLY.
        }
        tessera_ast::visitor::walk_expr(self, e);
    }
}
