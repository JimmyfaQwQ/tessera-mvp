//! Statement type checking (used by both Pass 3 body checking and Pass 4
//! top-level checking; the algorithm is the same in both contexts).

use tessera_ast::*;
use crate::{Type, FuncContext};
use indexmap::IndexMap;

use super::TypeChecker;

impl<'e> TypeChecker<'e> {
    pub(super) fn check_stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Let(l) => {
                let expected = l.ty.as_ref().map(|t| self.resolve_type(t));
                self.expected_ty = expected.clone();
                let actual = self.check_expr(&l.init);
                self.expected_ty = None;
                let ty = expected.unwrap_or(actual);
                self.env.define(l.name.name.clone(), ty);
            }
            Stmt::Assign(a) => {
                let rhs_ty = self.check_expr(&a.value);
                if let AssignTarget::Ident(i) = &a.target {
                    if let Some(existing) = self.env.lookup(&i.name).cloned() {
                        if !Self::types_compatible(&existing, &rhs_ty) {
                            self.env.error(
                                format!(
                                    "cannot assign {} to variable '{}' of type {}",
                                    rhs_ty, i.name, existing
                                ),
                                a.span,
                            );
                        }
                    }
                }
            }
            Stmt::If(i) => {
                let cond_ty = self.check_expr(&i.condition);
                if cond_ty != Type::Bool && cond_ty != Type::Error {
                    self.env.error(format!("if condition must be bool, got {cond_ty}"), i.condition.span());
                }
                self.check_block(&i.then_block);
                if let Some(eb) = &i.else_branch {
                    match eb {
                        ElseBranch::Else(b) => self.check_block(b),
                        ElseBranch::ElseIf(s) => self.check_stmt(&Stmt::If(*s.clone())),
                    }
                }
            }
            Stmt::While(w) => {
                let cond_ty = self.check_expr(&w.condition);
                if cond_ty != Type::Bool && cond_ty != Type::Error {
                    self.env.error(format!("while condition must be bool, got {cond_ty}"), w.condition.span());
                }
                self.check_block(&w.body);
            }
            Stmt::For(f) => {
                self.env.push_scope();
                if let Some(init) = &f.init { self.check_stmt(init); }
                if let Some(cond) = &f.condition { self.check_expr(cond); }
                if let Some(upd) = &f.update { self.check_stmt(upd); }
                self.check_block(&f.body);
                self.env.pop_scope();
            }
            Stmt::Return(r) => {
                if let Some(val) = &r.value { self.check_expr(val); }
            }
            Stmt::Break(_) | Stmt::Continue(_) => {}
            Stmt::ThreadSpawn(ts) => self.check_thread_spawn(ts),
            Stmt::ScopeBlock(sb) => {
                for arg in &sb.args { self.check_expr(arg); }
                let scope_bindings: Vec<(String, Type)> = match &sb.template {
                    ScopeTemplateRef::Named(ident) => {
                        if let Some((_id, info)) = self.env.lookup_template(&ident.name) {
                            info.params.iter()
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .chain(info.define_fields.iter().map(|(k, v)| (k.clone(), v.clone())))
                                .collect()
                        } else {
                            vec![]
                        }
                    }
                    ScopeTemplateRef::Anonymous(decl) => {
                        let mut b: Vec<(String, Type)> = decl.params.iter()
                            .map(|p| (p.name.name.clone(), self.resolve_type(&p.ty)))
                            .collect();
                        for m in &decl.members {
                            if let ScopeTemplateMember::Define(e) = m {
                                b.push((e.name.name.clone(), self.resolve_type(&e.ty)));
                            }
                        }
                        b
                    }
                };
                self.env.push_scope();
                let self_fields: IndexMap<String, Type> = match &sb.template {
                    ScopeTemplateRef::Named(ident) => {
                        if let Some((_id, info)) = self.env.lookup_template(&ident.name) {
                            let mut m: IndexMap<String, Type> = IndexMap::new();
                            for (k, v) in &info.params { m.insert(k.clone(), v.clone()); }
                            for (k, v) in &info.define_fields { m.insert(k.clone(), v.clone()); }
                            m
                        } else {
                            IndexMap::new()
                        }
                    }
                    ScopeTemplateRef::Anonymous(decl) => {
                        let mut m: IndexMap<String, Type> = IndexMap::new();
                        for p in &decl.params { m.insert(p.name.name.clone(), self.resolve_type(&p.ty)); }
                        for mem in &decl.members {
                            if let ScopeTemplateMember::Define(e) = mem {
                                m.insert(e.name.name.clone(), self.resolve_type(&e.ty));
                            }
                        }
                        m
                    }
                };
                self.env.define("self".to_string(), Type::TemplateObject(self_fields));
                for (name, ty) in scope_bindings {
                    self.env.define(name, ty);
                }
                for s in &sb.body.stmts { self.check_stmt(s); }
                self.env.pop_scope();
            }
            Stmt::ExclusiveBlock(eb) => {
                let old = self.env.in_exclusive;
                self.env.in_exclusive = true;
                self.check_block(&eb.body);
                self.env.in_exclusive = old;
            }
            Stmt::Expr(es) => { self.check_expr(&es.expr); }
        }
    }

    pub(super) fn check_thread_spawn(&mut self, ts: &ThreadSpawnStmt) {
        for arg in &ts.args { self.check_expr(arg); }
        if let HandleBind::Bind(name) = &ts.handle_bind {
            let handle_ty = match &ts.template {
                ThreadTemplateRef::Named(n) => {
                    if let Some((id, _)) = self.env.lookup_template(&n.name) {
                        Type::ThreadHandle(id)
                    } else {
                        self.env.error(format!("unknown thread template '{}'", n.name), n.span);
                        Type::Error
                    }
                }
                _ => Type::Error,
            };
            self.env.define(name.name.clone(), handle_ty);
        }
        let prev_func = std::mem::replace(
            &mut self.env.current_func,
            FuncContext::AsyncFunction { return_type: Type::Void },
        );
        self.check_block(&ts.body);
        self.env.current_func = prev_func;
    }
}
