//! L-AWAIT-UNCONSUMED-FUTURE (Warn): a bare statement whose value is a
//! `Future<R>` / `HandlerFuture<R>` that is dropped — never `.wait()`-ed,
//! `await`-ed, or bound (《Linter 规则草案 §12》). Such a Future typically never
//! runs (or its result is silently lost).
//!
//! Sound coverage (only fires when the producing type is *certain*):
//!  - a bare call to a user `async function` (returns `Future<T>`), resolved via
//!    `TypeEnv::func_sigs`;
//!  - a bare method call whose `ScopedTyper`-inferred return type is
//!    `Future`/`HandlerFuture`.
//! Excluded to avoid double-reporting: `terminate()` (owned by
//! L-TERMINATE-FUTURE-IGNORED) and handler dispatches on a thread handle (owned
//! by L-HANDLER-RESULT-IGNORED).

use tessera_ast::*;
use tessera_ast::visitor::Visitor;
use tessera_types::{Type, TypeEnv};

use crate::{Diagnostic, LintPass};
use super::scoped_visitor::ScopedTyper;

pub struct AwaitUnconsumedFuture;

impl LintPass for AwaitUnconsumedFuture {
    fn name(&self) -> &'static str { "L-AWAIT-UNCONSUMED-FUTURE" }
    fn check(&mut self, program: &Program, env: &TypeEnv) -> Vec<Diagnostic> {
        let mut v = V { env, typer: ScopedTyper::new(env), diags: vec![] };
        for item in &program.items {
            if let TopLevelItem::FuncDef(f) = item {
                v.visit_func_def(f);
            }
        }
        v.visit_program(program);
        v.diags
    }
}

struct V<'e> {
    env: &'e TypeEnv,
    typer: ScopedTyper<'e>,
    diags: Vec<Diagnostic>,
}

impl<'e> V<'e> {
    fn warn(&mut self, span: Span) {
        self.diags.push(
            Diagnostic::warn(
                "L-AWAIT-UNCONSUMED-FUTURE",
                "this Future is dropped without `.wait()` / `await` / binding; it may never run or its result is lost",
                span,
            )
            .with_help("bind it, or `.wait()` (sync) / `await` (async) it"),
        );
    }

    /// Decide whether a bare expression statement drops a Future/HandlerFuture.
    fn check_bare(&mut self, e: &Expr) {
        match e {
            // Bare call to a user `async function`.
            Expr::Call(c) => {
                if let Expr::Ident(f) = &c.callee {
                    if self.env.lookup_func_sig(&f.name).map(|s| s.is_async).unwrap_or(false) {
                        self.warn(c.span);
                    }
                }
            }
            Expr::MethodCall(m) => {
                if m.method.name == "terminate" {
                    return; // L-TERMINATE-FUTURE-IGNORED owns this.
                }
                // Handler dispatch on a thread handle is L-HANDLER-RESULT-IGNORED's job.
                if let Some(Type::ThreadHandle(id)) = self.typer.receiver_type(m) {
                    let is_handler = self.typer.template_by_id(id)
                        .map(|info| info.handlers.contains_key(&m.method.name))
                        .unwrap_or(false);
                    if is_handler { return; }
                }
                if matches!(self.typer.infer(e), Some(Type::Future(_)) | Some(Type::HandlerFuture(_))) {
                    self.warn(m.span);
                }
            }
            _ => {}
        }
    }
}

impl<'e> Visitor for V<'e> {
    fn visit_func_def(&mut self, f: &FuncDef) {
        self.typer.push_scope();
        for p in &f.params {
            let ty = self.typer.resolve_type(&p.ty);
            self.typer.define(p.name.name.clone(), ty);
        }
        tessera_ast::visitor::walk_func_def(self, f);
        self.typer.pop_scope();
    }

    fn visit_handler_def(&mut self, h: &HandlerDef) {
        self.typer.push_scope();
        for p in &h.params {
            let ty = self.typer.resolve_type(&p.ty);
            self.typer.define(p.name.name.clone(), ty);
        }
        tessera_ast::visitor::walk_handler_def(self, h);
        self.typer.pop_scope();
    }

    fn visit_block(&mut self, b: &Block) {
        self.typer.push_scope();
        tessera_ast::visitor::walk_block(self, b);
        self.typer.pop_scope();
    }

    fn visit_stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Let(l) => {
                self.visit_expr(&l.init);
                let ty = self.typer.let_type(l);
                self.typer.define(l.name.name.clone(), ty);
                return;
            }
            Stmt::For(f) => {
                self.typer.push_scope();
                if let Some(init) = &f.init { self.visit_stmt(init); }
                if let Some(c) = &f.condition { self.visit_expr(c); }
                if let Some(u) = &f.update { self.visit_stmt(u); }
                self.visit_block(&f.body);
                self.typer.pop_scope();
                return;
            }
            Stmt::ThreadSpawn(ts) => {
                for arg in &ts.args { self.visit_expr(arg); }
                self.visit_block(&ts.body);
                if let ThreadTemplateRef::Anonymous(decl) = &ts.template {
                    self.visit_thread_template_decl(decl);
                }
                if let HandleBind::Bind(name) = &ts.handle_bind {
                    if let ThreadTemplateRef::Named(tn) = &ts.template {
                        if let Some(id) = self.typer.lookup_template_id(&tn.name) {
                            self.typer.define(name.name.clone(), Type::ThreadHandle(id));
                        }
                    }
                }
                return;
            }
            Stmt::Expr(es) => {
                self.check_bare(&es.expr);
            }
            _ => {}
        }
        tessera_ast::visitor::walk_stmt(self, s);
    }
}
