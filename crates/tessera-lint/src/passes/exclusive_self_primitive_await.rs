use std::collections::HashSet;

use tessera_ast::*;
use tessera_ast::visitor::Visitor;
use tessera_types::{Type, TypeEnv};

use crate::{Diagnostic, LintPass};
use super::helpers::resolve_type_expr;

/// Warn: inside `#exclusive`, awaiting (or `.wait()`-ing) a synchronization
/// primitive that the current thread *owns* is a logic deadlock.
///
/// The sync-primitive model is "owner drives (`raise`/`fulfill`/`release`),
/// others await". If the owner thread blocks on its own primitive inside an
/// `#exclusive` block, it can no longer drive it — the wait only completes when
/// the owner terminates (Broken). This is the statically-detectable, sound
/// subset of R-EXCL-4 (《线程与事件循环规范 §4.5》).
///
/// Awaiting *another* thread's primitive inside `#exclusive` is legitimate
/// (that thread drives it — the case R-EXCL-4 explicitly blesses), so the pass
/// only fires for `self.<field>` where `<field>` is a primitive the enclosing
/// template declares via `expose` / `expose_mutable` / `define`.
pub struct ExclusiveSelfPrimitiveAwait;

impl LintPass for ExclusiveSelfPrimitiveAwait {
    fn name(&self) -> &'static str { "L-EXCL-AWAIT" }
    fn check(&mut self, program: &Program, env: &TypeEnv) -> Vec<Diagnostic> {
        let mut v = V { env, in_exclusive: 0, prims: vec![], diags: vec![] };
        v.visit_program(program);
        v.diags
    }
}

struct V<'e> {
    env: &'e TypeEnv,
    in_exclusive: usize,
    /// Stack of "self-owned sync-primitive field names" per enclosing thread
    /// template. The top entry describes the template `self` currently binds to.
    prims: Vec<HashSet<String>>,
    diags: Vec<Diagnostic>,
}

impl<'e> V<'e> {
    fn collect_prims(&self, d: &ThreadTemplateDecl) -> HashSet<String> {
        let mut set = HashSet::new();
        for m in &d.members {
            let decl = match m {
                ThreadTemplateMember::Expose(x)
                | ThreadTemplateMember::ExposeMutable(x)
                | ThreadTemplateMember::Define(x) => x,
                _ => continue,
            };
            if matches!(
                resolve_type_expr(self.env, &decl.ty),
                Type::Signal | Type::Contract | Type::Permit
            ) {
                set.insert(decl.name.name.clone());
            }
        }
        set
    }

    /// If `e` is `self.<field>` and `<field>` is a sync primitive owned by the
    /// current template, return that field name for diagnostic wording.
    fn self_owned_prim<'a>(&self, e: &'a Expr) -> Option<&'a str> {
        let fa = match e {
            Expr::FieldAccess(fa) => fa,
            _ => return None,
        };
        let obj = match &fa.object {
            Expr::Ident(i) => i,
            _ => return None,
        };
        if obj.name != "self" {
            return None;
        }
        let owned = self.prims.last()?.contains(&fa.field.name);
        if owned { Some(&fa.field.name) } else { None }
    }

    fn report(&mut self, field: &str, span: Span) {
        self.diags.push(
            Diagnostic::warn(
                "L-EXCL-AWAIT",
                format!(
                    "awaiting this thread's own synchronization primitive `self.{field}` inside `#exclusive` deadlocks: the owner is blocked here and can no longer drive it"
                ),
                span,
            )
            .with_help(
                "a primitive is driven by its owner (raise/fulfill/release) and awaited by others; await it outside `#exclusive`, or let another thread drive it (R-EXCL-4)",
            ),
        );
    }
}

impl<'e> Visitor for V<'e> {
    fn visit_thread_template_decl(&mut self, d: &ThreadTemplateDecl) {
        let prims = self.collect_prims(d);
        self.prims.push(prims);
        tessera_ast::visitor::walk_thread_template_decl(self, d);
        self.prims.pop();
    }

    fn visit_stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::ExclusiveBlock(eb) => {
                self.in_exclusive += 1;
                self.visit_block(&eb.body);
                self.in_exclusive -= 1;
            }
            Stmt::ThreadSpawn(ts) => {
                // The spawn body runs in the *spawned* thread: a different
                // `self`, and not inside this thread's `#exclusive`. Push an
                // empty owned-primitive set so we soundly detect nothing there
                // (the spawned thread's own members are visited with their real
                // context via the Anonymous decl / its top-level declaration).
                for arg in &ts.args { self.visit_expr(arg); }
                let saved = self.in_exclusive;
                self.in_exclusive = 0;
                self.prims.push(HashSet::new());
                self.visit_block(&ts.body);
                self.prims.pop();
                self.in_exclusive = saved;
                if let ThreadTemplateRef::Anonymous(decl) = &ts.template {
                    self.visit_thread_template_decl(decl);
                }
            }
            _ => tessera_ast::visitor::walk_stmt(self, s),
        }
    }

    fn visit_expr(&mut self, e: &Expr) {
        if self.in_exclusive > 0 {
            match e {
                Expr::Await(a) => {
                    if let Some(field) = self.self_owned_prim(&a.expr) {
                        let (field, span) = (field.to_string(), a.span);
                        self.report(&field, span);
                    }
                }
                Expr::MethodCall(m) if m.method.name == "wait" => {
                    if let Some(field) = self.self_owned_prim(&m.receiver) {
                        let (field, span) = (field.to_string(), m.span);
                        self.report(&field, span);
                    }
                }
                _ => {}
            }
        }
        tessera_ast::visitor::walk_expr(self, e);
    }
}
