//! `ScopedTyper`: a lint-local scope stack on top of `TypeEnv`.
//!
//! The type checker pops every function / handler scope after it has finished
//! type-checking that body, so locals bound by `let` are no longer visible
//! when lint passes run. `ScopedTyper` lets a pass maintain its own scope
//! stack while it walks the AST, mirroring `tessera-types::TypeEnv`'s
//! push / pop / define / lookup but staying lint-local.
//!
//! Usage from a `Visitor`:
//!
//! ```ignore
//! impl Visitor for MyVisitor<'_> {
//!     fn visit_block(&mut self, b: &Block) {
//!         self.typer.push_scope();
//!         walk_block(self, b);
//!         self.typer.pop_scope();
//!     }
//!     fn visit_stmt(&mut self, s: &Stmt) {
//!         if let Stmt::Let(l) = s {
//!             let ty = self.typer.let_type(l);
//!             self.typer.define(l.name.name.clone(), ty);
//!         }
//!         walk_stmt(self, s);
//!     }
//! }
//! ```
//!
//! The typer's `infer` mirrors `helpers::infer_expr_type` but consults the
//! scope stack first so locals introduced by `let` (including handler / func
//! parameters and `for`-init bindings) are visible.

use std::collections::HashMap;

use tessera_ast::*;
use tessera_types::{Type, TypeEnv};

use super::helpers::{infer_expr_type, resolve_type_expr};

pub(crate) struct ScopedTyper<'e> {
    env: &'e TypeEnv,
    scopes: Vec<HashMap<String, Type>>,
}

impl<'e> ScopedTyper<'e> {
    pub fn new(env: &'e TypeEnv) -> Self {
        Self { env, scopes: vec![HashMap::new()] }
    }

    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    pub fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    pub fn define(&mut self, name: String, ty: Type) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, ty);
        }
    }

    pub fn lookup(&self, name: &str) -> Option<Type> {
        for scope in self.scopes.iter().rev() {
            if let Some(ty) = scope.get(name) {
                return Some(ty.clone());
            }
        }
        self.env.lookup(name).cloned()
    }

    /// Resolve the type of a `let` statement: prefer the explicit annotation,
    /// otherwise fall back to inferring the initializer's type.
    pub fn let_type(&self, l: &LetStmt) -> Type {
        if let Some(te) = &l.ty {
            return resolve_type_expr(self.env, te);
        }
        self.infer(&l.init).unwrap_or(Type::Error)
    }

    /// Resolve a syntactic type expression (delegates to `helpers`).
    pub fn resolve_type(&self, te: &TypeExpr) -> Type {
        resolve_type_expr(self.env, te)
    }

    /// Lightweight expression inference that consults the scope stack first
    /// and the type-env-backed `helpers::infer_expr_type` as a fallback.
    pub fn infer(&self, e: &Expr) -> Option<Type> {
        match e {
            Expr::Ident(i) => self.lookup(&i.name),

            Expr::FieldAccess(fa) => {
                let obj = self.infer(&fa.object)?;
                match obj {
                    Type::ThreadHandle(id) => {
                        let info = self.env.templates.values()
                            .find(|(tid, _)| *tid == id)
                            .map(|(_, info)| info)?;
                        info.expose_fields.get(&fa.field.name).map(|ex| ex.ty.clone())
                    }
                    Type::TemplateObject(fields) => fields.get(&fa.field.name).cloned(),
                    _ => None,
                }
            }

            // For everything else delegate to the env-only inferer; the
            // pass-through is safe because none of the cases there depend on
            // scope-stack visibility (they recurse to Ident, which we've
            // already handled above by short-circuiting through scope stack).
            other => infer_expr_type(self.env, other),
        }
    }

    /// Returns a method-call's receiver-resolved Method-Call shape. Used by
    /// the affected passes to switch from `if let Expr::Ident = receiver` to a
    /// uniform "what's the receiver's type" check.
    pub fn receiver_type(&self, m: &MethodCallExpr) -> Option<Type> {
        self.infer(&m.receiver)
    }

    /// Look up a TemplateInfo by id. Used by passes that need to inspect a
    /// thread template's `handlers` / `expose_fields` after receiver-type
    /// resolution gave them a `Type::ThreadHandle(id)`.
    pub fn template_by_id(&self, id: usize) -> Option<&tessera_types::TemplateInfo> {
        self.env.templates.values()
            .find(|(tid, _)| *tid == id)
            .map(|(_, info)| info)
    }

    /// Look up a template's id by name. Used when defining the handle bound by
    /// `$Named(...) := h` so the typer knows `h: thread<Named>`.
    pub fn lookup_template_id(&self, name: &str) -> Option<usize> {
        self.env.templates.get(name).map(|(id, _)| *id)
    }
}
