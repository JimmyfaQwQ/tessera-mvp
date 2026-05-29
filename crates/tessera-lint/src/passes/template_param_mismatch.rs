//! L-AT-TEMPLATE-PARAM-MISMATCH (Warn): a template application
//! (`$Name(args) {..}` / `@Name(args) {..}`) whose argument count cannot match
//! the template's parameter list. Arity mismatch crashes the thread at runtime
//! (《模板与线程规范 §2》); this catches it statically.
//!
//! Bounds: `max` = total params, `min` = params without a default. Firing only
//! when `args < min` (definitely too few — required params unfilled) or
//! `args > max` (definitely too many) keeps the pass sound: a count in
//! `[min..=max]` is never flagged even if defaults are non-trailing.

use std::collections::HashMap;

use tessera_ast::*;
use tessera_ast::visitor::Visitor;
use tessera_types::TypeEnv;

use crate::{Diagnostic, LintPass};

/// Inclusive arity bounds for a template.
#[derive(Clone, Copy)]
struct Arity { min: usize, max: usize }

fn arity_of(params: &[Param]) -> Arity {
    Arity {
        min: params.iter().filter(|p| p.default.is_none()).count(),
        max: params.len(),
    }
}

pub struct TemplateParamMismatch;

impl LintPass for TemplateParamMismatch {
    fn name(&self) -> &'static str { "L-AT-TEMPLATE-PARAM-MISMATCH" }
    fn check(&mut self, program: &Program, _env: &TypeEnv) -> Vec<Diagnostic> {
        // name -> arity, gathered from the declarations (which carry defaults).
        let mut by_name: HashMap<String, Arity> = HashMap::new();
        for item in &program.items {
            match item {
                TopLevelItem::ScopeTemplateDecl(d) => {
                    if let Some(n) = &d.name { by_name.insert(n.name.clone(), arity_of(&d.params)); }
                }
                TopLevelItem::ThreadTemplateDecl(d) => {
                    if let Some(n) = &d.name { by_name.insert(n.name.clone(), arity_of(&d.params)); }
                }
                _ => {}
            }
        }

        let mut v = V { by_name, diags: vec![] };
        for item in &program.items {
            if let TopLevelItem::FuncDef(f) = item {
                v.visit_func_def(f);
            }
        }
        v.visit_program(program);
        v.diags
    }
}

struct V {
    by_name: HashMap<String, Arity>,
    diags: Vec<Diagnostic>,
}

impl V {
    fn check_apply(&mut self, kind: &str, name: &str, arity: Arity, n_args: usize, span: Span) {
        if n_args < arity.min || n_args > arity.max {
            let expected = if arity.min == arity.max {
                format!("{}", arity.max)
            } else {
                format!("{}..={}", arity.min, arity.max)
            };
            self.diags.push(
                Diagnostic::warn(
                    "L-AT-TEMPLATE-PARAM-MISMATCH",
                    format!("{kind} `{name}` applied with {n_args} argument(s), but it declares {expected} parameter(s)"),
                    span,
                )
                .with_help("match the argument count to the template's parameter list"),
            );
        }
    }
}

impl Visitor for V {
    fn visit_stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::ThreadSpawn(ts) => {
                let arity = match &ts.template {
                    ThreadTemplateRef::Named(n) => self.by_name.get(&n.name).copied()
                        .map(|a| (n.name.clone(), a)),
                    ThreadTemplateRef::Anonymous(decl) => Some((
                        decl.name.as_ref().map(|i| i.name.clone()).unwrap_or_else(|| "<anonymous>".into()),
                        arity_of(&decl.params),
                    )),
                    ThreadTemplateRef::Shorthand => None,
                };
                if let Some((name, a)) = arity {
                    self.check_apply("thread template", &name, a, ts.args.len(), ts.span);
                }
            }
            Stmt::ScopeBlock(sb) => {
                let arity = match &sb.template {
                    ScopeTemplateRef::Named(n) => self.by_name.get(&n.name).copied()
                        .map(|a| (n.name.clone(), a)),
                    ScopeTemplateRef::Anonymous(decl) => Some((
                        decl.name.as_ref().map(|i| i.name.clone()).unwrap_or_else(|| "<anonymous>".into()),
                        arity_of(&decl.params),
                    )),
                };
                if let Some((name, a)) = arity {
                    self.check_apply("scope template", &name, a, sb.args.len(), sb.span);
                }
            }
            _ => {}
        }
        // Keep walking to reach nested applies (in bodies, functions, etc.).
        tessera_ast::visitor::walk_stmt(self, s);
    }
}
