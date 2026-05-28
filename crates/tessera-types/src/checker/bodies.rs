//! Pass 3: type-check template member bodies (functions and handlers).
//!
//! Preconditions: Pass 2 (`registration.rs`) must have populated each
//! template's `expose_fields`, `define_fields`, and `params` so that the
//! synthetic `self: TemplateObject` binding resolves field accesses.

use tessera_ast::*;
use crate::{Type, FuncContext};
use indexmap::IndexMap;

use super::TypeChecker;

impl<'e> TypeChecker<'e> {
    pub(super) fn check_scope_template_bodies(&mut self, d: &ScopeTemplateDecl) {
        let mut self_fields: IndexMap<String, Type> = IndexMap::new();
        for p in &d.params {
            self_fields.insert(p.name.name.clone(), self.resolve_type(&p.ty));
        }
        for m in &d.members {
            if let ScopeTemplateMember::Define(e) = m {
                self_fields.insert(e.name.name.clone(), self.resolve_type(&e.ty));
            }
        }
        let self_binding = vec![("self".to_string(), Type::TemplateObject(self_fields))];
        for m in &d.members {
            let func = match m {
                ScopeTemplateMember::OnEnter(f) | ScopeTemplateMember::OnExit(f) | ScopeTemplateMember::MemberFunc(f) => f,
                ScopeTemplateMember::Define(_) => continue,
            };
            self.check_func_body(func, &self_binding);
        }
    }

    pub(super) fn check_thread_template_bodies(&mut self, d: &ThreadTemplateDecl) {
        let mut self_fields: IndexMap<String, Type> = IndexMap::new();
        for p in &d.params {
            self_fields.insert(p.name.name.clone(), self.resolve_type(&p.ty));
        }
        for m in &d.members {
            match m {
                ThreadTemplateMember::Expose(e) | ThreadTemplateMember::ExposeMutable(e) | ThreadTemplateMember::Define(e) => {
                    self_fields.insert(e.name.name.clone(), self.resolve_type(&e.ty));
                }
                _ => {}
            }
        }
        let self_binding = vec![("self".to_string(), Type::TemplateObject(self_fields))];

        for m in &d.members {
            match m {
                ThreadTemplateMember::OnEnter(f) | ThreadTemplateMember::OnExit(f)
                | ThreadTemplateMember::OnTerminate(f) | ThreadTemplateMember::MemberFunc(f) => {
                    self.check_func_body(f, &self_binding);
                }
                ThreadTemplateMember::Handler(h) => {
                    self.check_handler_body(h, &self_binding);
                }
                _ => {}
            }
        }
    }

    pub(super) fn check_func_body(&mut self, f: &FuncDef, extra_bindings: &[(String, Type)]) {
        let ret = self.resolve_type(&f.return_type);
        let ctx = if f.kind == FuncKind::Async {
            FuncContext::AsyncFunction { return_type: ret }
        } else {
            FuncContext::SyncFunction { return_type: ret }
        };
        let prev = std::mem::replace(&mut self.env.current_func, ctx);
        self.env.push_scope();
        for (name, ty) in extra_bindings { self.env.define(name.clone(), ty.clone()); }
        for p in &f.params { let ty = self.resolve_type(&p.ty); self.env.define(p.name.name.clone(), ty); }
        self.check_block(&f.body);
        self.env.pop_scope();
        self.env.current_func = prev;
    }

    pub(super) fn check_handler_body(&mut self, h: &HandlerDef, extra_bindings: &[(String, Type)]) {
        let ret = self.resolve_type(&h.return_type);
        let prev = std::mem::replace(&mut self.env.current_func, FuncContext::Handler { return_type: ret });
        self.env.push_scope();
        for (name, ty) in extra_bindings { self.env.define(name.clone(), ty.clone()); }
        for p in &h.params { let ty = self.resolve_type(&p.ty); self.env.define(p.name.name.clone(), ty); }
        self.check_block(&h.body);
        self.env.pop_scope();
        self.env.current_func = prev;
    }

    pub(super) fn check_block(&mut self, b: &Block) {
        self.env.push_scope();
        for s in &b.stmts { self.check_stmt(s); }
        self.env.pop_scope();
    }
}
