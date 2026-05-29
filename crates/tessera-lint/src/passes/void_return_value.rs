//! L-VOID-RETURN-VALUE: a function / handler / hook declared with return type
//! `void` must not `return <expr>;` — only a bare `return;` (or implicit
//! fall-through) is allowed. The spec defines this as an unconditional error
//! (《Linter 规则草案 §9》), so the check is purely syntactic and sound: any
//! `return` carrying a value inside a `void` unit fires.
//!
//! The complementary `L-RETURN-TYPE-MISMATCH` owns the non-void cases (bare
//! `return;` in a value-returning unit, plus literal type mismatches), so the
//! two passes never double-report the same `return`.
//!
//! A **thread spawn body** (`$Name(...) { ... }` / `${ ... }`) is also a
//! void-like context (《语句与控制流规范草案 §7》, void-like rule): a bare
//! `return;` ends the body early (= natural termination, R-LIFE-1), but
//! `return <expr>;` has no return type to satisfy and is flagged here too.

use tessera_ast::*;
use tessera_ast::visitor::Visitor;
use tessera_types::TypeEnv;

use crate::{Diagnostic, LintPass};
use super::helpers::{each_function, each_return, resolve_type_expr};

pub struct VoidReturnValue;

impl LintPass for VoidReturnValue {
    fn name(&self) -> &'static str { "L-VOID-RETURN-VALUE" }
    fn check(&mut self, program: &Program, env: &TypeEnv) -> Vec<Diagnostic> {
        let mut diags = Vec::new();
        each_function(program, &mut |ret_ty, body| {
            // Only `void` units are subject to this rule. `never` units (e.g. a
            // function returning `never`) are left to other diagnostics.
            if !matches!(resolve_type_expr(env, ret_ty), tessera_types::Type::Void) {
                return;
            }
            each_return(body, &mut |r| {
                if let Some(val) = &r.value {
                    diags.push(value_return_diag(
                        "a `void` function must not return a value; use a bare `return;`",
                        val.span(),
                    ));
                }
            });
        });

        // Thread-spawn bodies are void-like: flag `return <expr>;` directly in
        // the body (each_return skips nested spawn bodies, so each body is
        // checked exactly once as the visitor reaches its ThreadSpawn node).
        let mut v = SpawnBodyV { diags: &mut diags };
        for item in &program.items {
            if let TopLevelItem::FuncDef(f) = item {
                v.visit_func_def(f);
            }
        }
        v.visit_program(program);

        diags
    }
}

fn value_return_diag(msg: &str, span: Span) -> Diagnostic {
    Diagnostic::error("L-VOID-RETURN-VALUE", msg.to_string(), span)
        .with_help("remove the returned expression, or change the declared return type")
}

struct SpawnBodyV<'d> {
    diags: &'d mut Vec<Diagnostic>,
}

impl Visitor for SpawnBodyV<'_> {
    fn visit_stmt(&mut self, s: &Stmt) {
        if let Stmt::ThreadSpawn(ts) = s {
            each_return(&ts.body, &mut |r| {
                if let Some(val) = &r.value {
                    self.diags.push(value_return_diag(
                        "a thread body must not `return` a value; use a bare `return;` to end it early",
                        val.span(),
                    ));
                }
            });
        }
        // Keep walking to reach nested spawns (and spawns inside functions,
        // handlers, scope blocks, and other spawn bodies).
        tessera_ast::visitor::walk_stmt(self, s);
    }
}
