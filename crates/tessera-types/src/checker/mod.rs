//! Type checker for Tessera programs.
//!
//! # Pass structure
//!
//! `check_program` runs four passes in order:
//!
//! 1. **Pass 1 — name registration** (inline in `check_program`). Every
//!    template name is registered with placeholder info so that forward
//!    references (e.g. `thread<worker>` used before `worker` is declared)
//!    resolve during later passes.
//! 2. **Pass 1.5 — function signatures** (inline). Top-level function
//!    signatures are registered so call-site type-checking and recursive
//!    calls have a known return type.
//! 3. **Pass 2 — template resolution** (`registration.rs`). Each template's
//!    full `TemplateInfo` is computed by resolving member types.
//! 4. **Pass 3 — body checking** (`bodies.rs`). Template/handler bodies are
//!    type-checked using `stmts.rs` and `exprs.rs` helpers.
//! 5. **Pass 4 — top-level statements** (inline). Same as Pass 3 but for
//!    top-level statements outside any template.
//!
//! Each pass's preconditions:
//! - Pass 2 assumes Pass 1 registered every template name.
//! - Pass 3 assumes Pass 2 populated `TemplateInfo.expose_fields`,
//!   `params`, and `handlers` so `self.<field>` accesses can resolve.
//! - Pass 4 reuses Pass 3 helpers and assumes the env has all funcs/templates.

mod registration;
mod resolve;
mod bodies;
mod stmts;
mod exprs;

use tessera_ast::*;
use crate::{Type, TypeEnv, FuncSig, TemplateInfo, TemplateKind};
use indexmap::IndexMap;

pub struct TypeChecker<'e> {
    pub env: &'e mut TypeEnv,
    /// Expected return type for bidirectional Ok/Err/Some resolution.
    pub(super) expected_ty: Option<Type>,
}

impl<'e> TypeChecker<'e> {
    pub fn new(env: &'e mut TypeEnv) -> Self {
        Self { env, expected_ty: None }
    }

    // ── Program (orchestrator: passes 1, 1.5, 2, 3, 4) ───────────────────────

    pub fn check_program(&mut self, prog: &Program) {
        // Pass 1: register all template names with placeholder info so that
        // forward references are visible during type resolution in pass 2.
        for item in &prog.items {
            match item {
                TopLevelItem::ScopeTemplateDecl(d) => {
                    let name = d.name.as_ref().map(|i| i.name.clone()).unwrap_or_default();
                    if !name.is_empty() {
                        self.env.register_template(name, TemplateInfo {
                            kind: TemplateKind::Scope,
                            params: vec![],
                            define_fields: IndexMap::new(),
                            expose_fields: IndexMap::new(),
                            handlers: IndexMap::new(),
                            is_terminatable: false,
                        });
                    }
                }
                TopLevelItem::ThreadTemplateDecl(d) => {
                    let name = d.name.as_ref().map(|i| i.name.clone()).unwrap_or_default();
                    if !name.is_empty() {
                        self.env.register_template(name, TemplateInfo {
                            kind: TemplateKind::Thread,
                            params: vec![],
                            define_fields: IndexMap::new(),
                            expose_fields: IndexMap::new(),
                            handlers: IndexMap::new(),
                            is_terminatable: false,
                        });
                    }
                }
                TopLevelItem::FuncDef(_) | TopLevelItem::Statement(_) => {}
            }
        }
        // Pass 1.5: register top-level function signatures (enables call-site
        // type checks and gives recursive calls a return type to work with).
        for item in &prog.items {
            if let TopLevelItem::FuncDef(f) = item {
                let params: Vec<Type> = f.params.iter()
                    .map(|p| self.resolve_type(&p.ty))
                    .collect();
                let return_type = self.resolve_type(&f.return_type);
                let is_async = f.kind == FuncKind::Async;
                self.env.register_func_sig(f.name.name.clone(), FuncSig { params, return_type, is_async });
            }
        }
        // Pass 2: resolve types and fill in the full template info
        for item in &prog.items {
            match item {
                TopLevelItem::ScopeTemplateDecl(d) => self.register_scope_template(d),
                TopLevelItem::ThreadTemplateDecl(d) => self.register_thread_template(d),
                TopLevelItem::FuncDef(_) | TopLevelItem::Statement(_) => {}
            }
        }
        // Pass 3: type-check template member bodies
        for item in &prog.items {
            match item {
                TopLevelItem::ScopeTemplateDecl(d) => self.check_scope_template_bodies(d),
                TopLevelItem::ThreadTemplateDecl(d) => self.check_thread_template_bodies(d),
                TopLevelItem::FuncDef(_) | TopLevelItem::Statement(_) => {}
            }
        }
        // Pass 4: type-check top-level statements
        let top_stmts: Vec<&Stmt> = prog.items.iter()
            .filter_map(|item| match item {
                TopLevelItem::Statement(s) => Some(s),
                _ => None,
            })
            .collect();
        self.check_dup_lets(top_stmts.iter().copied());
        for s in &top_stmts {
            self.check_stmt(s);
        }
    }

    // ── Shared helpers (used across passes 3/4) ───────────────────────────────

    /// Returns true if `got` is acceptable where `expected` is declared.
    /// Silently passes `Type::Error` (already reported) and exact matches.
    pub(super) fn types_compatible(expected: &Type, got: &Type) -> bool {
        if Self::type_contains_error(got) || Self::type_contains_error(expected) { return true; }
        expected == got
    }

    pub(super) fn type_contains_error(t: &Type) -> bool {
        match t {
            Type::Error => true,
            Type::List(inner) | Type::Option(inner) | Type::Future(inner)
            | Type::HandlerFuture(inner) | Type::Locked(inner) | Type::Queue(inner) => {
                Self::type_contains_error(inner)
            }
            Type::Map(k, v) => Self::type_contains_error(k) || Self::type_contains_error(v),
            Type::Result(ok, err) => Self::type_contains_error(ok) || Self::type_contains_error(err),
            _ => false,
        }
    }
}
