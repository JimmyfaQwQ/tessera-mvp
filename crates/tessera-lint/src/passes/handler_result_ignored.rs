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
use super::scoped_visitor::ScopedTyper;

pub struct HandlerResultIgnored;

impl LintPass for HandlerResultIgnored {
    fn name(&self) -> &'static str { "L-HANDLER-RESULT-IGNORED" }
    fn check(&mut self, program: &Program, env: &TypeEnv) -> Vec<Diagnostic> {
        let mut v = V { typer: ScopedTyper::new(env), diags: vec![] };
        // walk_program skips top-level FuncDef bodies — walk them explicitly
        // so any handler-result-ignored pattern inside top-level async
        // functions is still seen.
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
    typer: ScopedTyper<'e>,
    diags: Vec<Diagnostic>,
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
                if let Expr::MethodCall(m) = &es.expr {
                    self.check_handler_call(m);
                }
            }
            _ => {}
        }
        tessera_ast::visitor::walk_stmt(self, s);
    }
}

impl<'e> V<'e> {
    fn check_handler_call(&mut self, m: &MethodCallExpr) {
        let Some(Type::ThreadHandle(id)) = self.typer.receiver_type(m) else { return };
        let method = m.method.name.as_str();
        if method == "terminate" || method == "__ping__" { return; }
        let is_handler = self.typer.template_by_id(id)
            .map(|info| info.handlers.contains_key(method))
            .unwrap_or(false);
        if !is_handler { return; }
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
