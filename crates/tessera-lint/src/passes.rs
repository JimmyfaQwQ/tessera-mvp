use tessera_ast::*;
use tessera_ast::visitor::Visitor;
use tessera_types::{Type, TypeEnv};
use crate::{Diagnostic, LintPass};

// ── L-AWAIT-ASYNC-ONLY ────────────────────────────────────────────────────────

pub struct AwaitAsyncOnly;

impl LintPass for AwaitAsyncOnly {
    fn name(&self) -> &'static str { "L-AWAIT-ASYNC-ONLY" }
    fn check(&mut self, program: &Program, _env: &TypeEnv) -> Vec<Diagnostic> {
        let mut v = AwaitAsyncVisitor { in_async: false, diags: vec![] };
        v.visit_program(program);
        v.diags
    }
}

struct AwaitAsyncVisitor {
    in_async: bool,
    diags: Vec<Diagnostic>,
}

impl Visitor for AwaitAsyncVisitor {
    fn visit_func_def(&mut self, f: &FuncDef) {
        let old = self.in_async;
        self.in_async = f.kind == FuncKind::Async;
        self.visit_block(&f.body);
        self.in_async = old;
    }

    fn visit_handler_def(&mut self, h: &HandlerDef) {
        let old = self.in_async;
        self.in_async = true;
        self.visit_block(&h.body);
        self.in_async = old;
    }

    fn visit_stmt(&mut self, s: &Stmt) {
        if let Stmt::ThreadSpawn(ts) = s {
            for arg in &ts.args { self.visit_expr(arg); }
            let old = self.in_async;
            self.in_async = true;
            self.visit_block(&ts.body);
            self.in_async = old;
            if let ThreadTemplateRef::Anonymous(decl) = &ts.template {
                self.visit_thread_template_decl(decl);
            }
        } else {
            tessera_ast::visitor::walk_stmt(self, s);
        }
    }

    fn visit_expr(&mut self, e: &Expr) {
        if let Expr::Await(a) = e {
            if !self.in_async {
                self.diags.push(
                    Diagnostic::error("L-AWAIT-ASYNC-ONLY", "await can only be used in async functions", a.span)
                        .with_help("make the enclosing function async, or use .wait() instead"),
                );
            }
        }
        tessera_ast::visitor::walk_expr(self, e);
    }
}

// ── L-HANDLER-MUST-ASYNC ──────────────────────────────────────────────────────

pub struct HandlerMustAsync;

impl LintPass for HandlerMustAsync {
    fn name(&self) -> &'static str { "L-HANDLER-MUST-ASYNC" }
    fn check(&mut self, _program: &Program, _env: &TypeEnv) -> Vec<Diagnostic> {
        // Handlers are always async by grammar; validated at parse time.
        vec![]
    }
}

// ── L-HANDLER-AWAIT-TYPE ──────────────────────────────────────────────────────

pub struct HandlerAwaitType;

impl LintPass for HandlerAwaitType {
    fn name(&self) -> &'static str { "L-HANDLER-AWAIT-TYPE" }
    fn check(&mut self, program: &Program, env: &TypeEnv) -> Vec<Diagnostic> {
        let mut v = HandlerAwaitVisitor { env, diags: vec![] };
        v.visit_program(program);
        v.diags
    }
}

struct HandlerAwaitVisitor<'e> {
    env: &'e TypeEnv,
    diags: Vec<Diagnostic>,
}

impl<'e> Visitor for HandlerAwaitVisitor<'e> {
    fn visit_expr(&mut self, e: &Expr) {
        if let Expr::MethodCall(m) = e {
            match m.method.name.as_str() {
                "wait" => {
                    // receiver should not be HandlerFuture
                    if let Expr::Ident(i) = &m.receiver {
                        if let Some(ty) = self.env.lookup(&i.name) {
                            if matches!(ty, Type::HandlerFuture(_)) {
                                self.diags.push(
                                    Diagnostic::error(
                                        "L-HANDLER-AWAIT-TYPE",
                                        "use .waitHandler() or .awaitHandler() on HandlerFuture, not .wait()",
                                        m.span,
                                    )
                                );
                            }
                        }
                    }
                }
                "waitHandler" | "awaitHandler" => {
                    // receiver should be HandlerFuture, not plain Future
                    if let Expr::Ident(i) = &m.receiver {
                        if let Some(ty) = self.env.lookup(&i.name) {
                            if matches!(ty, Type::Future(_)) {
                                self.diags.push(
                                    Diagnostic::error(
                                        "L-HANDLER-AWAIT-TYPE",
                                        "use .wait() or await on Future, not .waitHandler()/.awaitHandler()",
                                        m.span,
                                    )
                                );
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        tessera_ast::visitor::walk_expr(self, e);
    }
}

// ── L-EXPOSE-MUTABLE-UNSAFE ───────────────────────────────────────────────────

pub struct ExposeMutableUnsafe;

impl LintPass for ExposeMutableUnsafe {
    fn name(&self) -> &'static str { "L-EXPOSE-MUTABLE-UNSAFE" }
    fn check(&mut self, program: &Program, env: &TypeEnv) -> Vec<Diagnostic> {
        let mut diags = Vec::new();
        for item in &program.items {
            if let TopLevelItem::ThreadTemplateDecl(td) = item {
                for m in &td.members {
                    if let ThreadTemplateMember::ExposeMutable(e) = m {
                        let ty = resolve_type_expr(env, &e.ty);
                        if !ty.is_concurrent_safe() {
                            diags.push(
                                Diagnostic::error(
                                    "L-EXPOSE-MUTABLE-UNSAFE",
                                    format!("expose_mutable field '{}' has type '{}' which is not concurrent-safe; use locked<T> or Queue<T>", e.name.name, ty),
                                    e.span,
                                )
                            );
                        }
                    }
                }
            }
        }
        diags
    }
}

// ── L-GENERIC-TYPE-ARG-MISSING ────────────────────────────────────────────────

pub struct GenericTypeArgMissing;

impl LintPass for GenericTypeArgMissing {
    fn name(&self) -> &'static str { "L-GENERIC-TYPE-ARG-MISSING" }
    fn check(&mut self, program: &Program, _env: &TypeEnv) -> Vec<Diagnostic> {
        let mut v = GenericArgVisitor { diags: vec![] };
        v.visit_program(program);
        v.diags
    }
}

struct GenericArgVisitor {
    diags: Vec<Diagnostic>,
}

impl GenericArgVisitor {
    fn check_type(&mut self, te: &TypeExpr) {
        if let TypeExpr::Named(ident, args) = te {
            let required: Option<usize> = match ident.name.as_str() {
                "List" | "Option" | "Future" | "HandlerFuture" | "locked" | "Queue" => Some(1),
                "Map" | "Result" => Some(2),
                _ => None,
            };
            if let Some(n) = required {
                if args.len() != n {
                    self.diags.push(
                        Diagnostic::error(
                            "L-GENERIC-TYPE-ARG-MISSING",
                            format!("'{}' requires {} type argument(s), got {}", ident.name, n, args.len()),
                            ident.span,
                        )
                    );
                }
            }
            for a in args { self.check_type(a); }
        }
    }
}

impl Visitor for GenericArgVisitor {
    fn visit_type_expr(&mut self, t: &TypeExpr) {
        self.check_type(t);
    }

    fn visit_expose_decl(&mut self, e: &ExposeDecl, _mutable: bool) {
        self.check_type(&e.ty);
    }
}

// ── L-TERMINATE-NON-TERMINATABLE ─────────────────────────────────────────────

pub struct TerminateNonTerminatable;

impl LintPass for TerminateNonTerminatable {
    fn name(&self) -> &'static str { "L-TERMINATE-NON-TERMINATABLE" }
    fn check(&mut self, program: &Program, env: &TypeEnv) -> Vec<Diagnostic> {
        let mut v = TerminateVisitor { env, diags: vec![] };
        v.visit_program(program);
        v.diags
    }
}

struct TerminateVisitor<'e> {
    env: &'e TypeEnv,
    diags: Vec<Diagnostic>,
}

impl<'e> Visitor for TerminateVisitor<'e> {
    fn visit_expr(&mut self, e: &Expr) {
        if let Expr::MethodCall(m) = e {
            if m.method.name == "terminate" {
                if let Expr::Ident(i) = &m.receiver {
                    if let Some(Type::ThreadHandle(id)) = self.env.lookup(&i.name) {
                        for (_, (tid, info)) in &self.env.templates {
                            if tid == id && !info.is_terminatable {
                                self.diags.push(
                                    Diagnostic::error(
                                        "L-TERMINATE-NON-TERMINATABLE",
                                        format!("thread '{}' is not terminatable (no __on_terminate__ defined)", i.name),
                                        m.span,
                                    ).with_help("add 'async function __on_terminate__(): void { ... }' to the thread template")
                                );
                            }
                        }
                    }
                }
            }
        }
        tessera_ast::visitor::walk_expr(self, e);
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn resolve_type_expr(env: &TypeEnv, te: &TypeExpr) -> Type {
    match te {
        TypeExpr::Void => Type::Void,
        TypeExpr::Never => Type::Never,
        TypeExpr::Named(ident, args) => match ident.name.as_str() {
            "bool" => Type::Bool,
            "int" => Type::Int,
            "double" => Type::Double,
            "char" => Type::Char,
            "String" => Type::TString,
            "locked" => Type::Locked(Box::new(
                args.first().map(|a| resolve_type_expr(env, a)).unwrap_or(Type::Error)
            )),
            "Queue" => Type::Queue(Box::new(
                args.first().map(|a| resolve_type_expr(env, a)).unwrap_or(Type::Error)
            )),
            "signal" => Type::Signal,
            "contract" => Type::Contract,
            "permit" => Type::Permit,
            _ => Type::Error,
        },
    }
}

// ── L-PERMIT-AWAIT-IN-SYNC ────────────────────────────────────────────────────

/// Error: `.awaitPermit()` or `await permitExpr` called outside an async function.
pub struct PermitAwaitInSync;

impl LintPass for PermitAwaitInSync {
    fn name(&self) -> &'static str { "L-PERMIT-AWAIT-IN-SYNC" }
    fn check(&mut self, program: &Program, env: &TypeEnv) -> Vec<Diagnostic> {
        let mut v = PermitAwaitInSyncVisitor { env, in_async: false, diags: vec![] };
        v.visit_program(program);
        v.diags
    }
}

struct PermitAwaitInSyncVisitor<'e> {
    env: &'e TypeEnv,
    in_async: bool,
    diags: Vec<Diagnostic>,
}

impl<'e> Visitor for PermitAwaitInSyncVisitor<'e> {
    fn visit_func_def(&mut self, f: &FuncDef) {
        let old = self.in_async;
        self.in_async = f.kind == FuncKind::Async;
        self.visit_block(&f.body);
        self.in_async = old;
    }

    fn visit_handler_def(&mut self, h: &HandlerDef) {
        let old = self.in_async;
        self.in_async = true;
        self.visit_block(&h.body);
        self.in_async = old;
    }

    fn visit_stmt(&mut self, s: &Stmt) {
        if let Stmt::ThreadSpawn(ts) = s {
            for arg in &ts.args { self.visit_expr(arg); }
            let old = self.in_async;
            self.in_async = true;
            self.visit_block(&ts.body);
            self.in_async = old;
            if let ThreadTemplateRef::Anonymous(decl) = &ts.template {
                self.visit_thread_template_decl(decl);
            }
        } else {
            tessera_ast::visitor::walk_stmt(self, s);
        }
    }

    fn visit_expr(&mut self, e: &Expr) {
        if !self.in_async {
            // Check `.awaitPermit()` call on a permit receiver
            if let Expr::MethodCall(m) = e {
                if m.method.name == "awaitPermit" {
                    if let Expr::Ident(i) = &m.receiver {
                        if self.env.lookup(&i.name) == Some(&Type::Permit) {
                            self.diags.push(
                                Diagnostic::error(
                                    "L-PERMIT-AWAIT-IN-SYNC",
                                    ".awaitPermit() called in sync context; use .wait() instead",
                                    m.span,
                                )
                            );
                        }
                    }
                }
            }
            // `await permitExpr` in sync context is already caught by L-AWAIT-ASYNC-ONLY;
            // no duplicate needed here.
        }
        tessera_ast::visitor::walk_expr(self, e);
    }
}

// ── L-PERMIT-WAIT-IN-ASYNC ────────────────────────────────────────────────────

/// Warn: `.wait()` on a permit called inside an async function (blocks the thread).
pub struct PermitWaitInAsync;

impl LintPass for PermitWaitInAsync {
    fn name(&self) -> &'static str { "L-PERMIT-WAIT-IN-ASYNC" }
    fn check(&mut self, program: &Program, env: &TypeEnv) -> Vec<Diagnostic> {
        let mut v = PermitWaitInAsyncVisitor { env, in_async: false, diags: vec![] };
        v.visit_program(program);
        v.diags
    }
}

struct PermitWaitInAsyncVisitor<'e> {
    env: &'e TypeEnv,
    in_async: bool,
    diags: Vec<Diagnostic>,
}

impl<'e> Visitor for PermitWaitInAsyncVisitor<'e> {
    fn visit_func_def(&mut self, f: &FuncDef) {
        let old = self.in_async;
        self.in_async = f.kind == FuncKind::Async;
        self.visit_block(&f.body);
        self.in_async = old;
    }

    fn visit_handler_def(&mut self, h: &HandlerDef) {
        let old = self.in_async;
        self.in_async = true;
        self.visit_block(&h.body);
        self.in_async = old;
    }

    fn visit_stmt(&mut self, s: &Stmt) {
        if let Stmt::ThreadSpawn(ts) = s {
            for arg in &ts.args { self.visit_expr(arg); }
            let old = self.in_async;
            self.in_async = true;
            self.visit_block(&ts.body);
            self.in_async = old;
            if let ThreadTemplateRef::Anonymous(decl) = &ts.template {
                self.visit_thread_template_decl(decl);
            }
        } else {
            tessera_ast::visitor::walk_stmt(self, s);
        }
    }

    fn visit_expr(&mut self, e: &Expr) {
        if self.in_async {
            if let Expr::MethodCall(m) = e {
                if m.method.name == "wait" {
                    if let Expr::Ident(i) = &m.receiver {
                        if self.env.lookup(&i.name) == Some(&Type::Permit) {
                            self.diags.push(
                                Diagnostic::warn(
                                    "L-PERMIT-WAIT-IN-ASYNC",
                                    ".wait() on permit in async context blocks the thread; use .awaitPermit() or `await p`",
                                    m.span,
                                )
                            );
                        }
                    }
                }
            }
        }
        tessera_ast::visitor::walk_expr(self, e);
    }
}

// ── L-PERMIT-RELEASE-NON-POSITIVE ────────────────────────────────────────────

/// Error: `permit(initial)` with negative literal, or `release(n)` with non-positive literal.
pub struct PermitReleaseNonPositive;

impl LintPass for PermitReleaseNonPositive {
    fn name(&self) -> &'static str { "L-PERMIT-RELEASE-NON-POSITIVE" }
    fn check(&mut self, program: &Program, _env: &TypeEnv) -> Vec<Diagnostic> {
        let mut v = PermitReleaseVisitor { diags: vec![] };
        v.visit_program(program);
        v.diags
    }
}

struct PermitReleaseVisitor {
    diags: Vec<Diagnostic>,
}

impl Visitor for PermitReleaseVisitor {
    fn visit_expr(&mut self, e: &Expr) {
        match e {
            // permit(initial) with a negative literal
            Expr::Call(c) => {
                if let Expr::Ident(i) = &c.callee {
                    if i.name == "permit" {
                        if let Some(arg) = c.args.first() {
                            if let Expr::Lit(lit) = arg {
                                if let LitKind::Int(n) = lit.kind {
                                    if n < 0 {
                                        self.diags.push(
                                            Diagnostic::error(
                                                "L-PERMIT-RELEASE-NON-POSITIVE",
                                                format!("permit(initial): initial must be non-negative, got {n}"),
                                                c.span,
                                            )
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
            // p.release(n) with n <= 0
            Expr::MethodCall(m) => {
                if m.method.name == "release" {
                    if let Some(arg) = m.args.first() {
                        if let Expr::Lit(lit) = arg {
                            if let LitKind::Int(n) = lit.kind {
                                if n <= 0 {
                                    self.diags.push(
                                        Diagnostic::error(
                                            "L-PERMIT-RELEASE-NON-POSITIVE",
                                            format!("permit.release(n): n must be positive, got {n}"),
                                            m.span,
                                        )
                                    );
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        tessera_ast::visitor::walk_expr(self, e);
    }
}
