pub mod span;
pub mod visitor;

pub use span::Span;

// ── Identifiers ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

impl Ident {
    pub fn new(name: impl Into<String>, span: Span) -> Self {
        Self { name: name.into(), span }
    }
}

// ── Top-level ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Program {
    pub items: Vec<TopLevelItem>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum TopLevelItem {
    ScopeTemplateDecl(ScopeTemplateDecl),
    ThreadTemplateDecl(ThreadTemplateDecl),
    FuncDef(FuncDef),
    Statement(Stmt),
}

// ── Scope template (@template) ───────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ScopeTemplateDecl {
    pub name: Option<Ident>,
    pub params: Vec<Param>,
    pub members: Vec<ScopeTemplateMember>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ScopeTemplateMember {
    OnEnter(FuncDef),
    OnExit(FuncDef),
    MemberFunc(FuncDef),
}

// ── Thread template ($template) ──────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ThreadTemplateDecl {
    pub name: Option<Ident>,
    pub params: Vec<Param>,
    pub members: Vec<ThreadTemplateMember>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ThreadTemplateMember {
    OnEnter(FuncDef),
    OnExit(FuncDef),
    /// Presence of OnTerminate makes the thread terminatable.
    OnTerminate(FuncDef),
    MemberFunc(FuncDef),
    Handler(HandlerDef),
    Expose(ExposeDecl),
    ExposeMutable(ExposeDecl),
}

#[derive(Debug, Clone)]
pub struct HandlerDef {
    pub name: Ident,
    pub params: Vec<Param>,
    pub return_type: TypeExpr,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ExposeDecl {
    pub name: Ident,
    pub ty: TypeExpr,
    pub initializer: Option<Expr>,
    pub span: Span,
}

// ── Functions ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct FuncDef {
    pub kind: FuncKind,
    pub name: Ident,
    pub params: Vec<Param>,
    pub return_type: TypeExpr,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FuncKind {
    Sync,
    Async,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: Ident,
    pub ty: TypeExpr,
    pub default: Option<Expr>,
    pub span: Span,
}

// ── Statements ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Let(LetStmt),
    Assign(AssignStmt),
    If(IfStmt),
    While(WhileStmt),
    For(ForStmt),
    Return(ReturnStmt),
    Break(Span),
    Continue(Span),
    ThreadSpawn(ThreadSpawnStmt),
    ScopeBlock(ScopeBlockStmt),
    ExclusiveBlock(ExclusiveBlockStmt),
    Expr(ExprStmt),
}

#[derive(Debug, Clone)]
pub struct LetStmt {
    pub name: Ident,
    pub ty: Option<TypeExpr>,
    pub init: Expr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct AssignStmt {
    pub target: AssignTarget,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum AssignTarget {
    Ident(Ident),
    Field(Box<Expr>, Ident),
    Index(Box<Expr>, Box<Expr>),
}

impl AssignTarget {
    pub fn span(&self) -> Span {
        match self {
            AssignTarget::Ident(i) => i.span,
            AssignTarget::Field(e, f) => e.span().merge(f.span),
            AssignTarget::Index(e, i) => e.span().merge(i.span()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct IfStmt {
    pub condition: Expr,
    pub then_block: Block,
    pub else_branch: Option<ElseBranch>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ElseBranch {
    Else(Block),
    ElseIf(Box<IfStmt>),
}

#[derive(Debug, Clone)]
pub struct WhileStmt {
    pub condition: Expr,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ForStmt {
    pub init: Option<Box<Stmt>>,
    pub condition: Option<Expr>,
    pub update: Option<Box<Stmt>>,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ReturnStmt {
    pub value: Option<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ThreadSpawnStmt {
    pub template: ThreadTemplateRef,
    pub args: Vec<Expr>,
    pub body: Block,
    pub handle_bind: HandleBind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ThreadTemplateRef {
    Named(Ident),
    Anonymous(Box<ThreadTemplateDecl>),
    Shorthand,
}

#[derive(Debug, Clone)]
pub enum HandleBind {
    /// `let name = $Template(...) { ... }`  or  `... } := name`
    Bind(Ident),
    Discard,
}

#[derive(Debug, Clone)]
pub struct ScopeBlockStmt {
    pub template: ScopeTemplateRef,
    pub args: Vec<Expr>,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ScopeTemplateRef {
    Named(Ident),
    Anonymous(Box<ScopeTemplateDecl>),
}

#[derive(Debug, Clone)]
pub struct ExclusiveBlockStmt {
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ExprStmt {
    pub expr: Expr,
    pub span: Span,
}

// ── Expressions ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Expr {
    Lit(Literal),
    Ident(Ident),
    BinOp(Box<BinOpExpr>),
    UnaryOp(Box<UnaryOpExpr>),
    Call(Box<CallExpr>),
    MethodCall(Box<MethodCallExpr>),
    FieldAccess(Box<FieldAccessExpr>),
    Index(Box<IndexExpr>),
    Await(Box<AwaitExpr>),
    Panic(Box<PanicExpr>),
    Assert(Box<AssertExpr>),
    TypeCtor(Box<TypeCtorExpr>),
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Lit(l) => l.span,
            Expr::Ident(i) => i.span,
            Expr::BinOp(e) => e.span,
            Expr::UnaryOp(e) => e.span,
            Expr::Call(e) => e.span,
            Expr::MethodCall(e) => e.span,
            Expr::FieldAccess(e) => e.span,
            Expr::Index(e) => e.span,
            Expr::Await(e) => e.span,
            Expr::Panic(e) => e.span,
            Expr::Assert(e) => e.span,
            Expr::TypeCtor(e) => e.span,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Literal {
    pub kind: LitKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum LitKind {
    Bool(bool),
    Int(i64),
    Double(f64),
    Char(char),
    String(String),
    None,
}

#[derive(Debug, Clone)]
pub struct BinOpExpr {
    pub op: BinOp,
    pub left: Expr,
    pub right: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BinOp {
    Add, Sub, Mul, Div, Rem,
    Eq, Ne, Lt, Le, Gt, Ge,
    And, Or,
}

#[derive(Debug, Clone)]
pub struct UnaryOpExpr {
    pub op: UnaryOp,
    pub operand: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone)]
pub struct CallExpr {
    pub callee: Expr,
    pub args: Vec<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct MethodCallExpr {
    pub receiver: Expr,
    pub method: Ident,
    pub args: Vec<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct FieldAccessExpr {
    pub object: Expr,
    pub field: Ident,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct IndexExpr {
    pub object: Expr,
    pub index: Expr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct AwaitExpr {
    pub expr: Expr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct PanicExpr {
    pub message: Expr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct AssertExpr {
    pub condition: Expr,
    pub message: Option<Expr>,
    pub span: Span,
}

/// `Ok(expr)`, `Err(expr)`, `Some(expr)`, `List<int>()`, etc.
#[derive(Debug, Clone)]
pub struct TypeCtorExpr {
    pub name: String,
    pub type_args: Vec<TypeExpr>,
    pub args: Vec<Expr>,
    pub span: Span,
}

// ── Types ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeExpr {
    Named(Ident, Vec<TypeExpr>),
    Void,
    Never,
}

impl TypeExpr {
    pub fn span(&self) -> Option<Span> {
        match self {
            TypeExpr::Named(ident, _) => Some(ident.span),
            TypeExpr::Void | TypeExpr::Never => None,
        }
    }
}
