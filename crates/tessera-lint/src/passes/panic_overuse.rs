//! L-PANIC-OVERUSE (Info): a single function / handler that calls `panic(...)`
//! more than the configured threshold is flagged as a hint to model recoverable
//! failures with `Result<T, E>` / `Option<T>` instead (《Linter 规则草案 §7》,
//! 《错误与异常语义草案 §7》).
//!
//! The count is exact (every `panic(...)` in the unit's own body, not counting
//! nested thread-spawn bodies, which are their own units). The threshold is
//! deliberately conservative so the pass only fires on genuinely panic-heavy
//! code and never on ordinary validation.

use tessera_ast::*;

use tessera_types::TypeEnv;

use crate::{Diagnostic, LintPass};
use super::helpers::each_function;

/// Fire when a single unit contains strictly more than this many `panic(...)`.
const MAX_PANICS: usize = 3;

pub struct PanicOveruse;

impl LintPass for PanicOveruse {
    fn name(&self) -> &'static str { "L-PANIC-OVERUSE" }
    fn check(&mut self, program: &Program, _env: &TypeEnv) -> Vec<Diagnostic> {
        let mut diags = Vec::new();
        each_function(program, &mut |_ret, body| {
            let mut panics: Vec<Span> = Vec::new();
            collect_panics_block(body, &mut panics);
            if panics.len() > MAX_PANICS {
                let count = panics.len();
                // Report on the first panic so the diagnostic points somewhere useful.
                diags.push(
                    Diagnostic::info(
                        "L-PANIC-OVERUSE",
                        format!("this function calls `panic(...)` {count} times; consider returning `Result<T, E>` / `Option<T>` for recoverable failures"),
                        panics[0],
                    ),
                );
            }
        });
        diags
    }
}

fn collect_panics_block(b: &Block, out: &mut Vec<Span>) {
    for s in &b.stmts {
        collect_panics_stmt(s, out);
    }
}

fn collect_panics_stmt(s: &Stmt, out: &mut Vec<Span>) {
    match s {
        Stmt::Let(l) => collect_panics_expr(&l.init, out),
        Stmt::Assign(a) => collect_panics_expr(&a.value, out),
        Stmt::If(i) => {
            collect_panics_expr(&i.condition, out);
            collect_panics_block(&i.then_block, out);
            match &i.else_branch {
                Some(ElseBranch::Else(b)) => collect_panics_block(b, out),
                Some(ElseBranch::ElseIf(i2)) => collect_panics_stmt(&Stmt::If((**i2).clone()), out),
                None => {}
            }
        }
        Stmt::While(w) => { collect_panics_expr(&w.condition, out); collect_panics_block(&w.body, out); }
        Stmt::For(f) => {
            if let Some(init) = &f.init { collect_panics_stmt(init, out); }
            if let Some(c) = &f.condition { collect_panics_expr(c, out); }
            if let Some(u) = &f.update { collect_panics_stmt(u, out); }
            collect_panics_block(&f.body, out);
        }
        Stmt::Return(r) => { if let Some(v) = &r.value { collect_panics_expr(v, out); } }
        Stmt::ScopeBlock(sb) => {
            for a in &sb.args { collect_panics_expr(a, out); }
            collect_panics_block(&sb.body, out);
        }
        Stmt::ExclusiveBlock(eb) => collect_panics_block(&eb.body, out),
        Stmt::Expr(es) => collect_panics_expr(&es.expr, out),
        // ThreadSpawn body is a separate unit; Break / Continue carry no expr.
        _ => {}
    }
}

fn collect_panics_expr(e: &Expr, out: &mut Vec<Span>) {
    match e {
        Expr::Panic(p) => {
            out.push(p.message.span());
            collect_panics_expr(&p.message, out);
        }
        Expr::BinOp(b) => { collect_panics_expr(&b.left, out); collect_panics_expr(&b.right, out); }
        Expr::UnaryOp(u) => collect_panics_expr(&u.operand, out),
        Expr::Call(c) => { collect_panics_expr(&c.callee, out); for a in &c.args { collect_panics_expr(a, out); } }
        Expr::MethodCall(m) => { collect_panics_expr(&m.receiver, out); for a in &m.args { collect_panics_expr(a, out); } }
        Expr::FieldAccess(f) => collect_panics_expr(&f.object, out),
        Expr::Index(i) => { collect_panics_expr(&i.object, out); collect_panics_expr(&i.index, out); }
        Expr::Await(a) => collect_panics_expr(&a.expr, out),
        Expr::Try(t) => collect_panics_expr(&t.expr, out),
        Expr::Assert(a) => {
            collect_panics_expr(&a.condition, out);
            if let Some(m) = &a.message { collect_panics_expr(m, out); }
        }
        Expr::TypeCtor(tc) => { for a in &tc.args { collect_panics_expr(a, out); } }
        Expr::Lit(_) | Expr::Ident(_) => {}
    }
}
