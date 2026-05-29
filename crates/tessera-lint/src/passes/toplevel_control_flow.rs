//! L-TOPLEVEL-CONTROL-FLOW: the top-level execution context forbids `await`
//! (no enclosing async function / handler) and a bare `return` (no enclosing
//! function). `break` / `continue` placement is handled separately by
//! L-CONTROL-OUTSIDE-LOOP (which covers every context, not just the top level).

use tessera_ast::*;
use tessera_ast::visitor::Visitor;
use tessera_types::TypeEnv;
use crate::{Diagnostic, LintPass};

pub struct ToplevelControlFlow;

impl LintPass for ToplevelControlFlow {
    fn name(&self) -> &'static str { "L-TOPLEVEL-CONTROL-FLOW" }
    fn check(&mut self, program: &Program, _env: &TypeEnv) -> Vec<Diagnostic> {
        let mut v = ToplevelVisitor { in_toplevel: true, diags: vec![] };
        v.visit_program(program);
        v.diags
    }
}

struct ToplevelVisitor {
    in_toplevel: bool,
    diags: Vec<Diagnostic>,
}

impl ToplevelVisitor {
    fn push(&mut self, msg: &str, span: Span) {
        self.diags.push(
            Diagnostic::error("L-TOPLEVEL-CONTROL-FLOW", msg, span)
                .with_help("the top level executes outside any function or async context"),
        );
    }

    /// Run `f` with the top-level flag cleared (entering a function / handler /
    /// a freshly-spawned thread body).
    fn nested(&mut self, f: impl FnOnce(&mut Self)) {
        let saved = self.in_toplevel;
        self.in_toplevel = false;
        f(self);
        self.in_toplevel = saved;
    }
}

impl Visitor for ToplevelVisitor {
    fn visit_func_def(&mut self, f: &FuncDef) {
        self.nested(|v| tessera_ast::visitor::walk_func_def(v, f));
    }

    fn visit_handler_def(&mut self, h: &HandlerDef) {
        self.nested(|v| tessera_ast::visitor::walk_handler_def(v, h));
    }

    fn visit_stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Return(r) if self.in_toplevel => {
                self.push("`return` is not allowed at the top level", r.span);
            }
            Stmt::ThreadSpawn(ts) => {
                for arg in &ts.args { self.visit_expr(arg); }
                if let ThreadTemplateRef::Anonymous(decl) = &ts.template {
                    self.visit_thread_template_decl(decl);
                }
                // Spawn body runs in a fresh async thread — not the top level.
                self.nested(|v| v.visit_block(&ts.body));
                return;
            }
            _ => {}
        }
        tessera_ast::visitor::walk_stmt(self, s);
    }

    fn visit_expr(&mut self, e: &Expr) {
        if let Expr::Await(a) = e {
            if self.in_toplevel {
                self.push("`await` is not allowed at the top level (no enclosing async context)", a.span);
            }
        }
        tessera_ast::visitor::walk_expr(self, e);
    }
}
