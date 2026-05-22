use indexmap::IndexMap;

pub type TemplateId = usize;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    Bool,
    Int,
    Double,
    Char,
    TString,
    Void,
    Never,

    List(Box<Type>),
    Map(Box<Type>, Box<Type>),
    Option(Box<Type>),
    Result(Box<Type>, Box<Type>),
    Future(Box<Type>),
    HandlerFuture(Box<Type>),
    Locked(Box<Type>),
    Queue(Box<Type>),

    /// Broadcast manual-reset event.
    Signal,
    /// Auto-reset single-waiter (FIFO) event.
    Contract,

    ThreadHandle(TemplateId),

    HandlerDispatchError,

    /// Placeholder for recovery.
    Error,
}

impl Type {
    pub fn is_concurrent_safe(&self) -> bool {
        matches!(self, Type::Locked(_) | Type::Queue(_) | Type::Signal | Type::Contract)
    }

    pub fn is_numeric(&self) -> bool {
        matches!(self, Type::Int | Type::Double)
    }
}

impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::Bool => write!(f, "bool"),
            Type::Int => write!(f, "int"),
            Type::Double => write!(f, "double"),
            Type::Char => write!(f, "char"),
            Type::TString => write!(f, "String"),
            Type::Void => write!(f, "void"),
            Type::Never => write!(f, "never"),
            Type::List(t) => write!(f, "List<{t}>"),
            Type::Map(k, v) => write!(f, "Map<{k}, {v}>"),
            Type::Option(t) => write!(f, "Option<{t}>"),
            Type::Result(t, e) => write!(f, "Result<{t}, {e}>"),
            Type::Future(t) => write!(f, "Future<{t}>"),
            Type::HandlerFuture(t) => write!(f, "HandlerFuture<{t}>"),
            Type::Locked(t) => write!(f, "locked<{t}>"),
            Type::Queue(t) => write!(f, "Queue<{t}>"),
            Type::Signal => write!(f, "signal"),
            Type::Contract => write!(f, "contract"),
            Type::ThreadHandle(id) => write!(f, "ThreadHandle({id})"),
            Type::HandlerDispatchError => write!(f, "HandlerDispatchError"),
            Type::Error => write!(f, "<error>"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateKind {
    Scope,
    Thread,
}

#[derive(Debug, Clone)]
pub struct HandlerSig {
    pub params: Vec<(String, Type)>,
    pub return_type: Type,
}

#[derive(Debug, Clone)]
pub struct ExposeInfo {
    pub ty: Type,
    pub mutable: bool,
}

#[derive(Debug, Clone)]
pub struct TemplateInfo {
    pub kind: TemplateKind,
    pub params: Vec<(String, Type)>,
    /// Fields declared with `define` in a scope template — visible to the scope body.
    pub define_fields: IndexMap<String, Type>,
    pub expose_fields: IndexMap<String, ExposeInfo>,
    pub handlers: IndexMap<String, HandlerSig>,
    pub is_terminatable: bool,
}
