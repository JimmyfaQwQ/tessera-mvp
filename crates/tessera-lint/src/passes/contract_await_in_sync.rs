use tessera_ast::*;
use tessera_ast::visitor::Visitor;
use tessera_types::{Type, TypeEnv};
use crate::{Diagnostic, LintPass};

/// Error: `await <contract>` used outside an async function / handler.
/// Mirror of `SignalAwaitInSync` for `Type::Contract`.
pub struct ContractAwaitInSync;

impl LintPass for ContractAwaitInSync {
    fn name(&self) -> &'static str { "L-CONTRACT-AWAIT-IN-SYNC" }
    fn check(&mut self, program: &Program, env: &TypeEnv) -> Vec<Diagnostic> {
        let mut v = V { env, in_async: false, diags: vec![] };
        v.visit_program(program);
        v.diags
    }
}

struct V<'e> {
    env: &'e TypeEnv,
    in_async: bool,
    diags: Vec<Diagnostic>,
}

impl<'e> Visitor for V<'e> {
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
            if let Expr::Await(a) = e {
                if let Expr::Ident(i) = &a.expr {
                    if self.env.lookup(&i.name) == Some(&Type::Contract) {
                        self.diags.push(
                            Diagnostic::error(
                                "L-CONTRACT-AWAIT-IN-SYNC",
                                "`await <contract>` is only allowed inside an async function or handler",
                                a.span,
                            )
                            .with_help("use `.wait()` in sync contexts, or make the enclosing function async"),
                        );
                    }
                }
            }
        }
        tessera_ast::visitor::walk_expr(self, e);
    }
}
