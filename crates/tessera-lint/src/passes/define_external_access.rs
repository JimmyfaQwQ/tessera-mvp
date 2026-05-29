//! L-DEFINE-EXTERNAL-ACCESS: `define` fields are visible only inside the
//! declaring template's own members. Accessing them via a thread handle
//! from outside (e.g. `h.secret`) is forbidden by R-DEFINE-1 — and the
//! runtime already returns a generic "no field" error, but we surface it
//! statically.
//!
//! Because `define` fields are not registered in `TemplateInfo` (they
//! intentionally don't escape the template's own scope), this pass walks
//! the thread-template AST directly to gather the per-template define name
//! sets, then checks `FieldAccess` against them whenever the receiver type
//! is a `ThreadHandle`.

use std::collections::{HashMap, HashSet};

use tessera_ast::*;
use tessera_ast::visitor::Visitor;
use tessera_types::{Type, TypeEnv};
use crate::{Diagnostic, LintPass};

pub struct DefineExternalAccess;

impl LintPass for DefineExternalAccess {
    fn name(&self) -> &'static str { "L-DEFINE-EXTERNAL-ACCESS" }
    fn check(&mut self, program: &Program, env: &TypeEnv) -> Vec<Diagnostic> {
        // Build template_name -> define-field name set.
        let mut defines: HashMap<String, HashSet<String>> = HashMap::new();
        for item in &program.items {
            if let TopLevelItem::ThreadTemplateDecl(d) = item {
                let name = d.name.as_ref().map(|i| i.name.clone()).unwrap_or_default();
                if name.is_empty() { continue; }
                let mut set = HashSet::new();
                for m in &d.members {
                    if let ThreadTemplateMember::Define(e) = m {
                        set.insert(e.name.name.clone());
                    }
                }
                if !set.is_empty() {
                    defines.insert(name, set);
                }
            }
        }
        // Now map TemplateId -> define set via TypeEnv's template registry.
        let mut by_id: HashMap<usize, HashSet<String>> = HashMap::new();
        for (name, (id, _)) in &env.templates {
            if let Some(set) = defines.get(name) {
                by_id.insert(*id, set.clone());
            }
        }

        let mut v = V { env, by_id, diags: vec![] };
        v.visit_program(program);
        v.diags
    }
}

struct V<'e> {
    env: &'e TypeEnv,
    by_id: HashMap<usize, HashSet<String>>,
    diags: Vec<Diagnostic>,
}

impl<'e> Visitor for V<'e> {
    fn visit_expr(&mut self, e: &Expr) {
        if let Expr::FieldAccess(fa) = e {
            if let Expr::Ident(i) = &fa.object {
                if let Some(Type::ThreadHandle(id)) = self.env.lookup(&i.name) {
                    if let Some(set) = self.by_id.get(id) {
                        if set.contains(&fa.field.name) {
                            self.diags.push(
                                Diagnostic::error(
                                    "L-DEFINE-EXTERNAL-ACCESS",
                                    format!("`{}` is a `define` field — it is not accessible through the thread handle", fa.field.name),
                                    fa.span,
                                )
                                .with_help("expose the field with `expose` (read-only) or `expose_mutable` if external access is intended"),
                            );
                        }
                    }
                }
            }
        }
        tessera_ast::visitor::walk_expr(self, e);
    }
}
