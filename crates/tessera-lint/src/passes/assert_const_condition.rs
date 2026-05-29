//! L-ASSERT-ALWAYS-TRUE / L-ASSERT-ALWAYS-FALSE (Warn): an `assert(cond, ...)`
//! whose condition constant-folds to a fixed boolean (《Linter 规则草案 §7》).
//!
//! Folding uses **literals only** (`bool`/`int`/`double`/`char`/`String`) through
//! `!`, `&&`, `||`, and comparisons. If any leaf is non-literal (an identifier,
//! call, field access, …) the fold returns `None` and nothing fires — so the
//! pass only reports provably-constant conditions (zero false positives).
//! Arithmetic is intentionally not folded (kept simple; a miss, not a false fire).

use tessera_ast::*;
use tessera_ast::visitor::Visitor;
use tessera_types::TypeEnv;

use crate::{Diagnostic, LintPass};

pub struct AssertConstCondition;

impl LintPass for AssertConstCondition {
    fn name(&self) -> &'static str { "L-ASSERT-ALWAYS-TRUE / L-ASSERT-ALWAYS-FALSE" }
    fn check(&mut self, program: &Program, _env: &TypeEnv) -> Vec<Diagnostic> {
        let mut v = V { diags: vec![] };
        for item in &program.items {
            if let TopLevelItem::FuncDef(f) = item {
                v.visit_func_def(f);
            }
        }
        v.visit_program(program);
        v.diags
    }
}

/// A folded compile-time constant.
#[derive(Clone, PartialEq)]
enum Const { Bool(bool), Int(i64), Double(f64), Char(char), Str(String) }

fn fold(e: &Expr) -> Option<Const> {
    match e {
        Expr::Lit(l) => match &l.kind {
            LitKind::Bool(b) => Some(Const::Bool(*b)),
            LitKind::Int(n) => Some(Const::Int(*n)),
            LitKind::Double(d) => Some(Const::Double(*d)),
            LitKind::Char(c) => Some(Const::Char(*c)),
            LitKind::String(s) => Some(Const::Str(s.clone())),
            LitKind::None => None,
        },
        Expr::UnaryOp(u) => match (u.op.clone(), fold(&u.operand)?) {
            (UnaryOp::Not, Const::Bool(b)) => Some(Const::Bool(!b)),
            _ => None,
        },
        Expr::BinOp(b) => {
            let (l, r) = (fold(&b.left)?, fold(&b.right)?);
            match b.op {
                BinOp::And => match (l, r) {
                    (Const::Bool(a), Const::Bool(c)) => Some(Const::Bool(a && c)),
                    _ => None,
                },
                BinOp::Or => match (l, r) {
                    (Const::Bool(a), Const::Bool(c)) => Some(Const::Bool(a || c)),
                    _ => None,
                },
                BinOp::Eq => Some(Const::Bool(l == r)),
                BinOp::Ne => Some(Const::Bool(l != r)),
                BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                    let ord = compare(&l, &r)?;
                    Some(Const::Bool(match b.op {
                        BinOp::Lt => ord.is_lt(),
                        BinOp::Le => ord.is_le(),
                        BinOp::Gt => ord.is_gt(),
                        BinOp::Ge => ord.is_ge(),
                        _ => unreachable!(),
                    }))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Ordering of two same-typed comparable constants (`None` if not comparable).
fn compare(l: &Const, r: &Const) -> Option<std::cmp::Ordering> {
    match (l, r) {
        (Const::Int(a), Const::Int(b)) => Some(a.cmp(b)),
        (Const::Double(a), Const::Double(b)) => a.partial_cmp(b),
        (Const::Char(a), Const::Char(b)) => Some(a.cmp(b)),
        (Const::Str(a), Const::Str(b)) => Some(a.cmp(b)),
        _ => None,
    }
}

struct V { diags: Vec<Diagnostic> }

impl Visitor for V {
    fn visit_expr(&mut self, e: &Expr) {
        if let Expr::Assert(a) = e {
            if let Some(Const::Bool(b)) = fold(&a.condition) {
                if b {
                    self.diags.push(Diagnostic::warn(
                        "L-ASSERT-ALWAYS-TRUE",
                        "assert condition is always true; the assert is redundant",
                        a.condition.span(),
                    ));
                } else {
                    self.diags.push(Diagnostic::warn(
                        "L-ASSERT-ALWAYS-FALSE",
                        "assert condition is always false; this assert always fails",
                        a.condition.span(),
                    ).with_help("if this marks unreachable code, use `panic(\"...\")` instead"));
                }
            }
        }
        tessera_ast::visitor::walk_expr(self, e);
    }
}
