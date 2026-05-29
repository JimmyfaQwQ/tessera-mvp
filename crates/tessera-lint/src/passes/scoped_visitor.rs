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

            // MethodCall return-type inference for the methods lint passes
            // most often need in chains (Option / Result unwrap, container
            // pop / get, Future-style wait, etc.). Mirrors the relevant arms
            // of `checker::exprs::check_method_call` but stays lint-local so
            // it can run without re-typing the program.
            Expr::MethodCall(mc) => {
                let recv_ty = self.infer(&mc.receiver)?;
                method_return_type(&recv_ty, &mc.method.name, mc, self)
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

/// Return the type of `recv.method(args)` given the resolved receiver type
/// and the method name. Returns `None` for unknown / unsupported combos so
/// callers can conservatively skip. Mirrors the relevant cases of
/// `tessera_types::checker::exprs::check_method_call`.
fn method_return_type<'e>(
    recv: &Type,
    method: &str,
    mc: &MethodCallExpr,
    typer: &ScopedTyper<'e>,
) -> Option<Type> {
    match (method, recv) {
        ("length", Type::TString) | ("length", Type::List(_)) => Some(Type::Int),
        ("size", Type::Map(_, _)) | ("size", Type::Queue(_)) => Some(Type::Int),
        ("isEmpty", _) | ("isClosed", _) | ("isLocked", _)
        | ("isDone", _) | ("isOk", _) | ("isErr", _) | ("isSome", _) | ("isNone", _)
        | ("isRaised", _) | ("isPending", _)
        | ("startsWith", _) | ("endsWith", _) | ("contains", _)
        | ("isDigit", _) | ("isAlpha", _) | ("isWhitespace", _) | ("tryPush", _) => Some(Type::Bool),
        ("indexOf", _) | ("count", _) => Some(Type::Int),

        ("pop", Type::List(inner)) => Some(Type::Option(inner.clone())),
        ("tryPop", Type::Queue(inner)) | ("dequeue", Type::Queue(inner)) => {
            Some(Type::Option(inner.clone()))
        }
        ("get", Type::List(inner)) | ("get", Type::Locked(inner)) => Some(*inner.clone()),
        ("get", Type::Map(_, v)) => Some(Type::Option(v.clone())),
        ("remove", Type::Map(_, v)) => Some(Type::Option(v.clone())),

        ("unwrap", Type::Option(inner)) => Some(*inner.clone()),
        ("unwrap", Type::Result(ok, _)) => Some(*ok.clone()),
        ("unwrapErr", Type::Result(_, e)) => Some(*e.clone()),
        ("unwrapOr", Type::Option(inner)) => Some(*inner.clone()),
        ("unwrapOr", Type::Result(ok, _)) => Some(*ok.clone()),

        ("wait", Type::Future(inner)) | ("wait", Type::HandlerFuture(inner)) => {
            Some(*inner.clone())
        }
        ("wait", Type::Signal) | ("wait", Type::Contract) | ("wait", Type::Permit) => {
            Some(Type::Void)
        }

        ("trim", Type::TString) => Some(Type::TString),
        ("split", Type::TString) => Some(Type::List(Box::new(Type::TString))),
        ("toString", _) => Some(Type::TString),
        ("toInt", Type::TString) => Some(Type::Result(
            Box::new(Type::Int),
            Box::new(Type::ParseError),
        )),
        ("toInt", _) => Some(Type::Int),
        ("toDouble", Type::TString) => Some(Type::Result(
            Box::new(Type::Double),
            Box::new(Type::ParseError),
        )),
        ("toDouble", _) => Some(Type::Double),
        ("toChar", _) => Some(Type::Option(Box::new(Type::Char))),

        ("terminate", Type::ThreadHandle(_)) => Some(Type::Future(Box::new(Type::Void))),

        // A bare handler call on a thread handle resolves to its
        // HandlerFuture<R>; look up the template's handler signature.
        (name, Type::ThreadHandle(id)) => {
            let info = typer.template_by_id(*id)?;
            let sig = info.handlers.get(name)?;
            Some(Type::HandlerFuture(Box::new(sig.return_type.clone())))
        }

        _ => {
            // Silence "args unused" warning when we end up in the fall-through.
            let _ = mc;
            None
        }
    }
}
