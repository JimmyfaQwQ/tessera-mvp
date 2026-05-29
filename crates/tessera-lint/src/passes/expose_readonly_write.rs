//! L-EXPOSE-READONLY-WRITE: writing to an `expose` (non-mutable) field
//! through a thread handle is forbidden. The runtime would silently no-op
//! such an assignment; we flag it at compile time.

use tessera_ast::*;
use tessera_ast::visitor::Visitor;
use tessera_types::{Type, TypeEnv};
use crate::{Diagnostic, LintPass};

pub struct ExposeReadonlyWrite;

impl LintPass for ExposeReadonlyWrite {
    fn name(&self) -> &'static str { "L-EXPOSE-READONLY-WRITE" }
    fn check(&mut self, program: &Program, env: &TypeEnv) -> Vec<Diagnostic> {
        let mut v = V { env, diags: vec![] };
        v.visit_program(program);
        v.diags
    }
}

struct V<'e> {
    env: &'e TypeEnv,
    diags: Vec<Diagnostic>,
}

impl<'e> Visitor for V<'e> {
    fn visit_stmt(&mut self, s: &Stmt) {
        if let Stmt::Assign(a) = s {
            if let AssignTarget::Field(obj, field) = &a.target {
                if let Expr::Ident(i) = obj.as_ref() {
                    if let Some(Type::ThreadHandle(id)) = self.env.lookup(&i.name) {
                        let id = *id;
                        if let Some((_, info)) = self.env.templates.values()
                            .find(|(tid, _)| *tid == id)
                        {
                            if let Some(ex) = info.expose_fields.get(&field.name) {
                                if !ex.mutable {
                                    self.diags.push(
                                        Diagnostic::error(
                                            "L-EXPOSE-READONLY-WRITE",
                                            format!("`{}` is exposed read-only and cannot be assigned from outside", field.name),
                                            a.span,
                                        )
                                        .with_help("expose the field as `expose_mutable` with a concurrent-safe type, or mutate it via a handler"),
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
        tessera_ast::visitor::walk_stmt(self, s);
    }
}
