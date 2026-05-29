//! L-RETURN-TYPE-MISMATCH: the value carried by a `return` must match the
//! unit's declared return type, and a value-returning unit must not have a
//! bare `return;` (《Linter 规则草案 §9》).
//!
//! Soundness (zero false positives) is the hard constraint, so this pass only
//! fires on the two cases it can decide with certainty:
//!
//!  1. **Bare `return;` in a value-returning unit.** Purely syntactic: a unit
//!     whose declared return type is a concrete value type (not `void` /
//!     `never`) cannot satisfy a `return;` that carries no value. The type
//!     checker does not catch this, and `L-RETURN-NOT-ALL-PATHS` treats a bare
//!     `return` as a terminating path — so this is the rule that owns it.
//!
//!  2. **Literal return whose scalar type cannot match the declared type.** A
//!     scalar literal (`int` / `double` / `bool` / `char` / `String`) has a
//!     fully determined type regardless of context, so comparing it against a
//!     concrete declared type is exact. The one coercion the language tolerates
//!     — `int` ⇆ `double` — is conservatively skipped.
//!
//! Anything whose type depends on inference (calls, identifiers, operators,
//! constructors) is left alone: the type checker validates those, and guessing
//! here would risk a false positive. `void` units are handled by
//! `L-VOID-RETURN-VALUE`.

use tessera_ast::*;
use tessera_types::{Type, TypeEnv};

use crate::{Diagnostic, LintPass};
use super::helpers::{each_function, each_return, resolve_type_expr};

pub struct ReturnTypeMismatch;

impl LintPass for ReturnTypeMismatch {
    fn name(&self) -> &'static str { "L-RETURN-TYPE-MISMATCH" }
    fn check(&mut self, program: &Program, env: &TypeEnv) -> Vec<Diagnostic> {
        let mut diags = Vec::new();
        each_function(program, &mut |ret_ty, body| {
            let declared = resolve_type_expr(env, ret_ty);
            // `void` is L-VOID-RETURN-VALUE's job; `never` units and any type we
            // could not resolve are left alone to keep this pass sound.
            if matches!(declared, Type::Void | Type::Never | Type::Error) {
                return;
            }
            each_return(body, &mut |r| {
                match &r.value {
                    None => {
                        diags.push(
                            Diagnostic::error(
                                "L-RETURN-TYPE-MISMATCH",
                                format!("`return;` carries no value, but the function is declared to return `{declared}`"),
                                r.span,
                            )
                            .with_help("return a value of the declared type"),
                        );
                    }
                    Some(val) => {
                        if let Some(lit) = literal_scalar_type(val) {
                            if !compatible(&declared, &lit) {
                                diags.push(
                                    Diagnostic::error(
                                        "L-RETURN-TYPE-MISMATCH",
                                        format!("returns `{lit}` but the function is declared to return `{declared}`"),
                                        val.span(),
                                    )
                                    .with_help("return a value of the declared type, or change the declared return type"),
                                );
                            }
                        }
                    }
                }
            });
        });
        diags
    }
}

/// The fully-determined scalar type of a literal, or `None` for any expression
/// whose type is context-dependent (and therefore not safe to judge here).
fn literal_scalar_type(e: &Expr) -> Option<Type> {
    match e {
        Expr::Lit(l) => match l.kind {
            LitKind::Bool(_) => Some(Type::Bool),
            LitKind::Int(_) => Some(Type::Int),
            LitKind::Double(_) => Some(Type::Double),
            LitKind::Char(_) => Some(Type::Char),
            LitKind::String(_) => Some(Type::TString),
            // `None` is an `Option<_>` with an unknown inner type — skip.
            LitKind::None => None,
        },
        _ => None,
    }
}

/// Whether a scalar literal of type `lit` may satisfy declared type `declared`.
/// Exact equality, plus the language's tolerated `int` ⇆ `double` coercion.
fn compatible(declared: &Type, lit: &Type) -> bool {
    if declared == lit {
        return true;
    }
    matches!(
        (declared, lit),
        (Type::Int, Type::Double) | (Type::Double, Type::Int)
    )
}
