use tessera_ast::*;
use tessera_types::TypeEnv;
use crate::{Diagnostic, LintPass};
use super::helpers::resolve_type_expr;

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
