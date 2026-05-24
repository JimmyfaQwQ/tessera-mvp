use indexmap::IndexMap;
use crate::ty::{Type, TemplateInfo};

#[derive(Debug, Clone)]
pub struct FuncSig {
    pub params: Vec<Type>,
    pub return_type: Type,
    pub is_async: bool,
}

#[derive(Debug, Clone)]
pub struct Scope {
    pub bindings: IndexMap<String, Type>,
}

impl Scope {
    pub fn new() -> Self { Self { bindings: IndexMap::new() } }
    pub fn define(&mut self, name: String, ty: Type) { self.bindings.insert(name, ty); }
    pub fn lookup(&self, name: &str) -> Option<&Type> { self.bindings.get(name) }
}

#[derive(Debug, Clone)]
pub enum FuncContext {
    TopLevel,
    SyncFunction { return_type: Type },
    AsyncFunction { return_type: Type },
    Handler { return_type: Type },
}

impl FuncContext {
    pub fn is_async(&self) -> bool {
        matches!(self, FuncContext::AsyncFunction { .. } | FuncContext::Handler { .. })
    }

    pub fn return_type(&self) -> Option<&Type> {
        match self {
            FuncContext::TopLevel => None,
            FuncContext::SyncFunction { return_type }
            | FuncContext::AsyncFunction { return_type }
            | FuncContext::Handler { return_type } => Some(return_type),
        }
    }
}

#[derive(Debug)]
pub struct TypeEnv {
    pub scopes: Vec<Scope>,
    pub templates: IndexMap<String, (crate::TemplateId, TemplateInfo)>,
    pub func_sigs: IndexMap<String, FuncSig>,
    pub current_func: FuncContext,
    pub in_exclusive: bool,
    pub diagnostics: Vec<TypeDiagnostic>,
    next_template_id: usize,
}

impl TypeEnv {
    pub fn new() -> Self {
        Self {
            scopes: vec![Scope::new()],
            templates: IndexMap::new(),
            func_sigs: IndexMap::new(),
            current_func: FuncContext::TopLevel,
            in_exclusive: false,
            diagnostics: Vec::new(),
            next_template_id: 0,
        }
    }

    pub fn push_scope(&mut self) { self.scopes.push(Scope::new()); }
    pub fn pop_scope(&mut self) { self.scopes.pop(); }

    pub fn define(&mut self, name: String, ty: Type) {
        self.scopes.last_mut().unwrap().define(name, ty);
    }

    pub fn lookup(&self, name: &str) -> Option<&Type> {
        for scope in self.scopes.iter().rev() {
            if let Some(ty) = scope.lookup(name) { return Some(ty); }
        }
        None
    }

    pub fn register_func_sig(&mut self, name: String, sig: FuncSig) {
        self.func_sigs.insert(name, sig);
    }

    pub fn lookup_func_sig(&self, name: &str) -> Option<&FuncSig> {
        self.func_sigs.get(name)
    }

    pub fn register_template(&mut self, name: String, info: TemplateInfo) -> crate::TemplateId {
        let id = self.next_template_id;
        self.next_template_id += 1;
        self.templates.insert(name, (id, info));
        id
    }

    /// Update an already-registered template's info in place, preserving its id.
    pub fn update_template(&mut self, name: &str, info: TemplateInfo) {
        if let Some((_, existing)) = self.templates.get_mut(name) {
            *existing = info;
        }
    }

    pub fn lookup_template(&self, name: &str) -> Option<(crate::TemplateId, &TemplateInfo)> {
        self.templates.get(name).map(|(id, info)| (*id, info))
    }

    pub fn error(&mut self, msg: impl Into<String>, span: tessera_ast::Span) {
        self.diagnostics.push(TypeDiagnostic { message: msg.into(), span });
    }
}

#[derive(Debug, Clone, thiserror::Error, miette::Diagnostic)]
#[error("{message}")]
#[diagnostic(code(tessera::typecheck))]
pub struct TypeDiagnostic {
    pub message: String,
    #[label("type error occurs here")]
    pub span: tessera_ast::Span,
}
