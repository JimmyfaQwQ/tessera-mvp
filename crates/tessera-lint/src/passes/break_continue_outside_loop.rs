//! L-CONTROL-OUTSIDE-LOOP: `break` / `continue` are only legal inside a `while`
//! / `for` loop (《语句与控制流规范草案 §6.1 / §6.2》). The parser accepts them
//! anywhere and the interpreter then exits the body oddly; this pass rejects any
//! `break`/`continue` with no enclosing loop, in *every* context — top level,
//! functions, handlers, hooks, and spawn bodies.
//!
//! Loop context is tracked with an `in_loop` depth that:
//!   - increments inside `while`/`for` bodies,
//!   - resets to 0 at function / handler / thread-spawn boundaries (a loop does
//!     not cross into a nested function or a freshly-spawned thread),
//!   - is transparent to `if` / scope / `#exclusive` blocks (those run inline, so
//!     a `break` inside them legitimately targets the surrounding loop).

use tessera_ast::*;
use tessera_ast::visitor::Visitor;
use tessera_types::TypeEnv;

use crate::{Diagnostic, LintPass};

pub struct BreakContinueOutsideLoop;

impl LintPass for BreakContinueOutsideLoop {
    fn name(&self) -> &'static str { "L-CONTROL-OUTSIDE-LOOP" }
    fn check(&mut self, program: &Program, _env: &TypeEnv) -> Vec<Diagnostic> {
        let mut v = V { in_loop: 0, diags: vec![] };
        // walk_program skips top-level FuncDef bodies — visit them explicitly so
        // a `break;` inside a free function is still caught.
        for item in &program.items {
            if let TopLevelItem::FuncDef(f) = item {
                v.visit_func_def(f);
            }
        }
        v.visit_program(program);
        v.diags
    }
}

struct V {
    in_loop: usize,
    diags: Vec<Diagnostic>,
}

impl V {
    fn report(&mut self, kw: &str, span: Span) {
        self.diags.push(
            Diagnostic::error(
                "L-CONTROL-OUTSIDE-LOOP",
                format!("`{kw}` is only allowed inside a `while` or `for` loop"),
                span,
            )
            .with_help("remove it, or place it inside a loop body"),
        );
    }

    /// Run `f` with the loop context reset (entering a function / handler / a
    /// freshly-spawned thread body).
    fn in_fresh_context(&mut self, f: impl FnOnce(&mut Self)) {
        let saved = self.in_loop;
        self.in_loop = 0;
        f(self);
        self.in_loop = saved;
    }
}

impl Visitor for V {
    fn visit_func_def(&mut self, f: &FuncDef) {
        self.in_fresh_context(|v| v.visit_block(&f.body));
    }

    fn visit_handler_def(&mut self, h: &HandlerDef) {
        self.in_fresh_context(|v| v.visit_block(&h.body));
    }

    fn visit_stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Break(span) if self.in_loop == 0 => self.report("break", *span),
            Stmt::Continue(span) if self.in_loop == 0 => self.report("continue", *span),
            Stmt::While(w) => {
                self.visit_expr(&w.condition);
                self.in_loop += 1;
                self.visit_block(&w.body);
                self.in_loop -= 1;
            }
            Stmt::For(f) => {
                if let Some(init) = &f.init { self.visit_stmt(init); }
                if let Some(c) = &f.condition { self.visit_expr(c); }
                if let Some(u) = &f.update { self.visit_stmt(u); }
                self.in_loop += 1;
                self.visit_block(&f.body);
                self.in_loop -= 1;
            }
            Stmt::ThreadSpawn(ts) => {
                for arg in &ts.args { self.visit_expr(arg); }
                // The spawn body runs in a new thread — its own loop context.
                self.in_fresh_context(|v| v.visit_block(&ts.body));
                if let ThreadTemplateRef::Anonymous(decl) = &ts.template {
                    self.visit_thread_template_decl(decl);
                }
            }
            // if / scope / #exclusive blocks are transparent to loop context;
            // the default walk preserves `in_loop`.
            _ => tessera_ast::visitor::walk_stmt(self, s),
        }
    }
}
