//! L-FUNCTION-HOOK-SIGNATURE: enforce hook signatures.
//!
//! * `__on_enter__` and `__on_exit__` must be SYNC `function`s with no
//!   parameters and a `void` return type.
//! * `__on_terminate__` must be an ASYNC `function` with no parameters and
//!   a `void` return type.
//!
//! The runtime depends on these shapes — calling a hook with the wrong arity
//! crashes the thread with a generic "param mismatch" message; this lint
//! turns that into an actionable up-front error.

use tessera_ast::*;
use tessera_types::TypeEnv;
use crate::{Diagnostic, LintPass};

pub struct HookSignature;

impl LintPass for HookSignature {
    fn name(&self) -> &'static str { "L-FUNCTION-HOOK-SIGNATURE" }
    fn check(&mut self, program: &Program, _env: &TypeEnv) -> Vec<Diagnostic> {
        let mut diags = Vec::new();
        // Note: the parser is permissive about hook misclassification —
        // e.g. `function __on_terminate__()` (sync, illegal) parses as a
        // generic MemberFunc, not OnTerminate. We therefore inspect every
        // member function by name so we catch the wrong-kind cases too.
        let check_by_name = |f: &FuncDef, diags: &mut Vec<Diagnostic>| {
            match f.name.name.as_str() {
                "__on_enter__" | "__on_exit__" => check_sync_void(f, &f.name.name.clone(), diags),
                "__on_terminate__" => check_async_void(f, "__on_terminate__", diags),
                _ => {}
            }
        };
        for item in &program.items {
            match item {
                TopLevelItem::ScopeTemplateDecl(d) => {
                    for m in &d.members {
                        if let ScopeTemplateMember::OnEnter(f)
                            | ScopeTemplateMember::OnExit(f)
                            | ScopeTemplateMember::MemberFunc(f) = m
                        {
                            check_by_name(f, &mut diags);
                        }
                    }
                }
                TopLevelItem::ThreadTemplateDecl(d) => {
                    for m in &d.members {
                        if let ThreadTemplateMember::OnEnter(f)
                            | ThreadTemplateMember::OnExit(f)
                            | ThreadTemplateMember::OnTerminate(f)
                            | ThreadTemplateMember::MemberFunc(f) = m
                        {
                            check_by_name(f, &mut diags);
                        }
                    }
                }
                _ => {}
            }
        }
        diags
    }
}

fn check_sync_void(f: &FuncDef, name: &str, diags: &mut Vec<Diagnostic>) {
    if f.kind != FuncKind::Sync {
        diags.push(err(format!("hook `{name}` must be a synchronous `function`, not `async`"), f.span));
    }
    if !f.params.is_empty() {
        diags.push(err(format!("hook `{name}` must take no parameters"), f.span));
    }
    if !is_void(&f.return_type) {
        diags.push(err(format!("hook `{name}` must return `void`"), f.span));
    }
}

fn check_async_void(f: &FuncDef, name: &str, diags: &mut Vec<Diagnostic>) {
    if f.kind != FuncKind::Async {
        diags.push(err(format!("hook `{name}` must be declared `async function`"), f.span));
    }
    if !f.params.is_empty() {
        diags.push(err(format!("hook `{name}` must take no parameters"), f.span));
    }
    if !is_void(&f.return_type) {
        diags.push(err(format!("hook `{name}` must return `void`"), f.span));
    }
}

fn is_void(t: &TypeExpr) -> bool {
    matches!(t, TypeExpr::Void)
}

fn err(msg: String, span: Span) -> Diagnostic {
    Diagnostic::error("L-FUNCTION-HOOK-SIGNATURE", msg, span)
        .with_help("hook signatures are fixed by the runtime; see the template & thread spec for the required forms")
}
