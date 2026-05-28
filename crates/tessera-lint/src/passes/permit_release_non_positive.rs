use tessera_ast::*;
use tessera_ast::visitor::Visitor;
use tessera_types::TypeEnv;
use crate::{Diagnostic, LintPass};

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
                        if let Some(Expr::Lit(lit)) = c.args.first() {
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
            // p.release(n) with n <= 0
            Expr::MethodCall(m) => {
                if m.method.name == "release" {
                    if let Some(Expr::Lit(lit)) = m.args.first() {
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
            _ => {}
        }
        tessera_ast::visitor::walk_expr(self, e);
    }
}
