use tessera_ast::*;
use tessera_ast::visitor::Visitor;
use tessera_types::TypeEnv;
use crate::{Diagnostic, LintPass};

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
