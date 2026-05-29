//! L-RETURN-NOT-ALL-PATHS: a non-void function must `return` (or terminate
//! via `panic(...)`) on every path. Loops are conservatively assumed to
//! possibly skip their body, so a `return` only inside a loop does not
//! satisfy the requirement.
//!
//! Algorithm: `definitely_terminates(block)` is `true` iff at least one
//! statement in the block definitely terminates execution. For an `if/else`
//! both branches must definitely terminate; a bare `if` without `else`
//! never definitely terminates because the false branch falls through.

use tessera_ast::*;
use tessera_types::TypeEnv;
use crate::{Diagnostic, LintPass};

pub struct ReturnNotAllPaths;

impl LintPass for ReturnNotAllPaths {
    fn name(&self) -> &'static str { "L-RETURN-NOT-ALL-PATHS" }
    fn check(&mut self, program: &Program, _env: &TypeEnv) -> Vec<Diagnostic> {
        let mut diags = Vec::new();
        for item in &program.items {
            match item {
                TopLevelItem::FuncDef(f) => check_func(f, &mut diags),
                TopLevelItem::ScopeTemplateDecl(d) => {
                    for m in &d.members {
                        if let ScopeTemplateMember::MemberFunc(f) = m {
                            check_func(f, &mut diags);
                        }
                    }
                }
                TopLevelItem::ThreadTemplateDecl(d) => {
                    for m in &d.members {
                        match m {
                            ThreadTemplateMember::MemberFunc(f) => check_func(f, &mut diags),
                            ThreadTemplateMember::Handler(h) => check_handler(h, &mut diags),
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        diags
    }
}

fn check_func(f: &FuncDef, diags: &mut Vec<Diagnostic>) {
    if matches!(f.return_type, TypeExpr::Void | TypeExpr::Never) {
        return;
    }
    if !definitely_terminates(&f.body) {
        diags.push(
            Diagnostic::error(
                "L-RETURN-NOT-ALL-PATHS",
                format!("function `{}` does not return on every path", f.name.name),
                f.span,
            )
            .with_help("add a `return` (or `panic(...)`) at the end of every branch, or change the return type to `void`"),
        );
    }
}

fn check_handler(h: &HandlerDef, diags: &mut Vec<Diagnostic>) {
    if matches!(h.return_type, TypeExpr::Void | TypeExpr::Never) {
        return;
    }
    if !definitely_terminates(&h.body) {
        diags.push(
            Diagnostic::error(
                "L-RETURN-NOT-ALL-PATHS",
                format!("handler `{}` does not return on every path", h.name.name),
                h.span,
            )
            .with_help("add a `return` (or `panic(...)`) at the end of every branch"),
        );
    }
}

fn definitely_terminates(block: &Block) -> bool {
    for s in &block.stmts {
        if stmt_terminates(s) {
            return true;
        }
    }
    false
}

fn stmt_terminates(s: &Stmt) -> bool {
    match s {
        Stmt::Return(_) => true,
        Stmt::Expr(es) => matches!(&es.expr, Expr::Panic(_)),
        Stmt::If(i) => {
            if let Some(eb) = &i.else_branch {
                let then_term = definitely_terminates(&i.then_block);
                let else_term = match eb {
                    ElseBranch::Else(b) => definitely_terminates(b),
                    ElseBranch::ElseIf(i2) => stmt_terminates(&Stmt::If((**i2).clone())),
                };
                then_term && else_term
            } else {
                false
            }
        }
        // Loops, blocks, etc. are conservatively assumed to fall through.
        _ => false,
    }
}
