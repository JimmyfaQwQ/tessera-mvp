//! Pass 2: resolve each template's full `TemplateInfo` after Pass 1 has
//! reserved every template's name.

use tessera_ast::*;
use crate::{TemplateInfo, TemplateKind, HandlerSig, ExposeInfo, Type};
use indexmap::IndexMap;

use super::TypeChecker;

impl<'e> TypeChecker<'e> {
    pub(super) fn register_scope_template(&mut self, d: &ScopeTemplateDecl) {
        let name = d.name.as_ref().map(|i| i.name.clone()).unwrap_or_default();
        if name.is_empty() { return; }
        let params = d.params.iter().map(|p| (p.name.name.clone(), self.resolve_type(&p.ty))).collect();
        let mut define_fields = IndexMap::new();
        for m in &d.members {
            if let ScopeTemplateMember::Define(e) = m {
                define_fields.insert(e.name.name.clone(), self.resolve_type(&e.ty));
            }
        }
        let info = TemplateInfo {
            kind: TemplateKind::Scope,
            params,
            define_fields,
            expose_fields: IndexMap::new(),
            handlers: IndexMap::new(),
            is_terminatable: false,
        };
        self.env.update_template(&name, info);
    }

    pub(super) fn register_thread_template(&mut self, d: &ThreadTemplateDecl) {
        let name = d.name.as_ref().map(|i| i.name.clone()).unwrap_or_default();
        if name.is_empty() { return; }
        let params = d.params.iter().map(|p| (p.name.name.clone(), self.resolve_type(&p.ty))).collect();
        let mut expose_fields = IndexMap::new();
        let mut handlers = IndexMap::new();
        let mut is_terminatable = false;

        for m in &d.members {
            match m {
                ThreadTemplateMember::OnTerminate(_) => is_terminatable = true,
                ThreadTemplateMember::Handler(h) => {
                    let sig = HandlerSig {
                        params: h.params.iter().map(|p| (p.name.name.clone(), self.resolve_type(&p.ty))).collect(),
                        return_type: self.resolve_type(&h.return_type),
                    };
                    handlers.insert(h.name.name.clone(), sig);
                }
                ThreadTemplateMember::Expose(e) => {
                    expose_fields.insert(e.name.name.clone(), ExposeInfo { ty: self.resolve_type(&e.ty), mutable: false });
                }
                ThreadTemplateMember::ExposeMutable(e) => {
                    expose_fields.insert(e.name.name.clone(), ExposeInfo { ty: self.resolve_type(&e.ty), mutable: true });
                }
                _ => {}
            }
        }

        // R-HANDLER-PING: every thread template implicitly carries
        // `async handler __ping__(): String`. We register it unconditionally so
        // the type checker sees `handle.__ping__()` as legal; the runtime
        // intercepts at dispatch and returns "pong" without ever queuing the
        // call (see ThreadState::dispatch_handler). A user-declared __ping__ is
        // overwritten here on purpose — lint L-HANDLER-PING-REDEFINED is the
        // surface that tells the user not to do this.
        handlers.insert(
            "__ping__".to_string(),
            HandlerSig { params: vec![], return_type: Type::TString },
        );

        let info = TemplateInfo {
            kind: TemplateKind::Thread,
            params,
            define_fields: IndexMap::new(),
            expose_fields,
            handlers,
            is_terminatable,
        };
        self.env.update_template(&name, info);
    }
}
