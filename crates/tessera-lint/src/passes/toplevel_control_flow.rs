//! L-TOPLEVEL-CONTROL-FLOW: top-level execution context forbids
//! `await` (no enclosing async function / handler) and bare `return` /
//! `break` / `continue` (no enclosing function or loop). Break / continue
//! inside top-level `for` / `while` remain legal because they target the
//! surrounding loop.

use tessera_ast::*;
use tessera_ast::visitor::Visitor;
use tessera_types::TypeEnv;
use crate::{Diagnostic, LintPass};

pub struct ToplevelControlFlow;

impl LintPass for ToplevelControlFlow {
    fn name(&self) -> &'static str { "L-TOPLEVEL-CONTROL-FLOW" }
    fn check(&mut self, program: &Program, _env: &TypeEnv) -> Vec<Diagnostic> {
        let mut v = ToplevelVisitor { in_toplevel: true, in_loop: false, diags: vec![] };
        v.visit_program(program);
        v.diags
    }
}

struct ToplevelVisitor {
    in_toplevel: bool,
    in_loop: bool,
    diags: Vec<Diagnostic>,
}

impl ToplevelVisitor {
    fn push(&mut self, msg: &str, span: Span) {
        self.diags.push(
            Diagnostic::error("L-TOPLEVEL-CONTROL-FLOW", msg, span)
                .with_help("the top level executes outside any function, loop, or async context"),
        );
    }
}

impl Visitor for ToplevelVisitor {
    fn visit_func_def(&mut self, f: &FuncDef) {
        let old_top = self.in_toplevel;
        let old_loop = self.in_loop;
        self.in_toplevel = false;
        self.in_loop = false;
        tessera_ast::visitor::walk_func_def(self, f);
        self.in_loop = old_loop;
        self.in_toplevel = old_top;
    }

    fn visit_handler_def(&mut self, h: &HandlerDef) {
        let old_top = self.in_toplevel;
        let old_loop = self.in_loop;
        self.in_toplevel = false;
        self.in_loop = false;
        tessera_ast::visitor::walk_handler_def(self, h);
        self.in_loop = old_loop;
        self.in_toplevel = old_top;
    }

    fn visit_stmt(&mut self, s: &Stmt) {
        // Track loop / spawn boundaries before recursing.
        match s {
            Stmt::Return(r) if self.in_toplevel => {
                self.push("`return` is not allowed at the top level", r.span);
            }
            Stmt::Break(span) if self.in_toplevel && !self.in_loop => {
                self.push("`break` at the top level must be inside a `for` or `while` loop", *span);
            }
            Stmt::Continue(span) if self.in_toplevel && !self.in_loop => {
                self.push("`continue` at the top level must be inside a `for` or `while` loop", *span);
            }
            Stmt::While(w) => {
                self.visit_expr(&w.condition);
                let old_loop = self.in_loop;
                self.in_loop = true;
                self.visit_block(&w.body);
                self.in_loop = old_loop;
                return;
            }
            Stmt::For(f) => {
                if let Some(init) = &f.init { self.visit_stmt(init); }
                if let Some(c) = &f.condition { self.visit_expr(c); }
                if let Some(u) = &f.update { self.visit_stmt(u); }
                let old_loop = self.in_loop;
                self.in_loop = true;
                self.visit_block(&f.body);
                self.in_loop = old_loop;
                return;
            }
            Stmt::ThreadSpawn(ts) => {
                for arg in &ts.args { self.visit_expr(arg); }
                if let ThreadTemplateRef::Anonymous(decl) = &ts.template {
                    self.visit_thread_template_decl(decl);
                }
                // Spawn body executes in a fresh async thread — leave both
                // the top-level and loop contexts behind.
                let old_top = self.in_toplevel;
                let old_loop = self.in_loop;
                self.in_toplevel = false;
                self.in_loop = false;
                self.visit_block(&ts.body);
                self.in_loop = old_loop;
                self.in_toplevel = old_top;
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
