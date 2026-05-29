use tessera_ast::*;
use tessera_ast::visitor::Visitor;
use tessera_types::{Type, TypeEnv};
use crate::{Diagnostic, LintPass};
use super::scoped_visitor::ScopedTyper;

pub struct HandlerAwaitType;

impl LintPass for HandlerAwaitType {
    fn name(&self) -> &'static str { "L-HANDLER-AWAIT-TYPE" }
    fn check(&mut self, program: &Program, env: &TypeEnv) -> Vec<Diagnostic> {
        let mut v = V { typer: ScopedTyper::new(env), diags: vec![] };
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
            _ => {}
        }
        tessera_ast::visitor::walk_stmt(self, s);
    }

    fn visit_expr(&mut self, e: &Expr) {
        if let Expr::MethodCall(m) = e {
            let recv_ty = self.typer.receiver_type(m);
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
