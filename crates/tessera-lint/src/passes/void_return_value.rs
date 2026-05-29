//! L-VOID-RETURN-VALUE: a function / handler / hook declared with return type
//! `void` must not `return <expr>;` — only a bare `return;` (or implicit
//! fall-through) is allowed. The spec defines this as an unconditional error
//! (《Linter 规则草案 §9》), so the check is purely syntactic and sound: any
//! `return` carrying a value inside a `void` unit fires.
//!
//! The complementary `L-RETURN-TYPE-MISMATCH` owns the non-void cases (bare
//! `return;` in a value-returning unit, plus literal type mismatches), so the
//! two passes never double-report the same `return`.

use tessera_ast::*;
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
                    diags.push(
                        Diagnostic::error(
                            "L-VOID-RETURN-VALUE",
                            "a `void` function must not return a value; use a bare `return;`",
                            val.span(),
                        )
                        .with_help("remove the returned expression, or change the declared return type"),
                    );
                }
            });
        });
        diags
    }
}
