use tessera_ast::*;
use tessera_ast::visitor::Visitor;
use tessera_types::TypeEnv;
use crate::{Diagnostic, LintPass};

pub struct AwaitAsyncOnly;

impl LintPass for AwaitAsyncOnly {
    fn name(&self) -> &'static str { "L-AWAIT-ASYNC-ONLY" }
    fn check(&mut self, program: &Program, _env: &TypeEnv) -> Vec<Diagnostic> {
        let mut v = AwaitAsyncVisitor { in_async: false, diags: vec![] };
        v.visit_program(program);
        v.diags
    }
}

struct AwaitAsyncVisitor {
    in_async: bool,
    diags: Vec<Diagnostic>,
}

impl Visitor for AwaitAsyncVisitor {
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
        if let Expr::Await(a) = e {
            if !self.in_async {
                self.diags.push(
                    Diagnostic::error("L-AWAIT-ASYNC-ONLY", "await can only be used in async functions", a.span)
                        .with_help("make the enclosing function async, or use .wait() instead"),
                );
            }
        }
        tessera_ast::visitor::walk_expr(self, e);
    }
}
