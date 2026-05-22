use tessera_ast::*;
use crate::{Type, TypeEnv, FuncContext, TemplateInfo, TemplateKind, HandlerSig, ExposeInfo};
use indexmap::IndexMap;

pub struct TypeChecker<'e> {
    pub env: &'e mut TypeEnv,
    /// Expected return type for bidirectional Ok/Err/Some resolution.
    expected_ty: Option<Type>,
}

impl<'e> TypeChecker<'e> {
    pub fn new(env: &'e mut TypeEnv) -> Self {
        Self { env, expected_ty: None }
    }

    // ── Program ───────────────────────────────────────────────────────────────

    pub fn check_program(&mut self, prog: &Program) {
        // First pass: register all top-level template names
        for item in &prog.items {
            match item {
                TopLevelItem::ScopeTemplateDecl(d) => self.register_scope_template(d),
                TopLevelItem::ThreadTemplateDecl(d) => self.register_thread_template(d),
                TopLevelItem::FuncDef(_) | TopLevelItem::Statement(_) => {}
            }
        }
        // Second pass: type-check statements
        for item in &prog.items {
            if let TopLevelItem::Statement(s) = item {
                self.check_stmt(s);
            }
        }
    }

    // ── Template registration ─────────────────────────────────────────────────

    fn register_scope_template(&mut self, d: &ScopeTemplateDecl) {
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
        self.env.register_template(name, info);
    }

    fn register_thread_template(&mut self, d: &ThreadTemplateDecl) {
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

        let info = TemplateInfo { kind: TemplateKind::Thread, params, define_fields: IndexMap::new(), expose_fields, handlers, is_terminatable };
        self.env.register_template(name, info);
    }

    // ── Type resolution ───────────────────────────────────────────────────────

    pub fn resolve_type(&self, te: &TypeExpr) -> Type {
        match te {
            TypeExpr::Void => Type::Void,
            TypeExpr::Never => Type::Never,
            TypeExpr::Named(ident, args) => self.resolve_named_type(&ident.name, args, ident.span),
        }
    }

    fn resolve_named_type(&self, name: &str, args: &[TypeExpr], _span: Span) -> Type {
        match name {
            "bool"   => Type::Bool,
            "int"    => Type::Int,
            "double" => Type::Double,
            "char"   => Type::Char,
            "String" => Type::TString,
            "void"   => Type::Void,
            "never"  => Type::Never,
            "List"   => {
                let inner = args.first().map(|a| self.resolve_type(a)).unwrap_or(Type::Error);
                Type::List(Box::new(inner))
            }
            "Map" => {
                let k = args.first().map(|a| self.resolve_type(a)).unwrap_or(Type::Error);
                let v = args.get(1).map(|a| self.resolve_type(a)).unwrap_or(Type::Error);
                Type::Map(Box::new(k), Box::new(v))
            }
            "Option" => {
                let inner = args.first().map(|a| self.resolve_type(a)).unwrap_or(Type::Error);
                Type::Option(Box::new(inner))
            }
            "Result" => {
                let t = args.first().map(|a| self.resolve_type(a)).unwrap_or(Type::Error);
                let e = args.get(1).map(|a| self.resolve_type(a)).unwrap_or(Type::Error);
                Type::Result(Box::new(t), Box::new(e))
            }
            "Future" => {
                let inner = args.first().map(|a| self.resolve_type(a)).unwrap_or(Type::Error);
                Type::Future(Box::new(inner))
            }
            "HandlerFuture" => {
                let inner = args.first().map(|a| self.resolve_type(a)).unwrap_or(Type::Error);
                Type::HandlerFuture(Box::new(inner))
            }
            "locked" => {
                let inner = args.first().map(|a| self.resolve_type(a)).unwrap_or(Type::Error);
                Type::Locked(Box::new(inner))
            }
            "Queue" => {
                let inner = args.first().map(|a| self.resolve_type(a)).unwrap_or(Type::Error);
                Type::Queue(Box::new(inner))
            }
            "thread" => {
                // thread<TemplateName> — the type arg names the template
                if let Some(TypeExpr::Named(ident, _)) = args.first() {
                    if let Some((id, _)) = self.env.lookup_template(&ident.name) {
                        return Type::ThreadHandle(id);
                    }
                }
                Type::Error
            }
            other => {
                // Bare template name used directly as a type (legacy / shorthand)
                if let Some((id, _)) = self.env.lookup_template(other) {
                    Type::ThreadHandle(id)
                } else {
                    Type::Error
                }
            }
        }
    }

    // ── Statements ────────────────────────────────────────────────────────────

    fn check_stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Let(l) => {
                let expected = l.ty.as_ref().map(|t| self.resolve_type(t));
                self.expected_ty = expected.clone();
                let actual = self.check_expr(&l.init);
                self.expected_ty = None;
                let ty = expected.unwrap_or(actual);
                self.env.define(l.name.name.clone(), ty);
            }
            Stmt::Assign(a) => { self.check_expr(&a.value); }
            Stmt::If(i) => {
                let cond_ty = self.check_expr(&i.condition);
                if cond_ty != Type::Bool && cond_ty != Type::Error {
                    self.env.error(format!("if condition must be bool, got {cond_ty}"), i.condition.span());
                }
                self.check_block(&i.then_block);
                if let Some(eb) = &i.else_branch {
                    match eb {
                        ElseBranch::Else(b) => self.check_block(b),
                        ElseBranch::ElseIf(s) => self.check_stmt(&Stmt::If(*s.clone())),
                    }
                }
            }
            Stmt::While(w) => {
                let cond_ty = self.check_expr(&w.condition);
                if cond_ty != Type::Bool && cond_ty != Type::Error {
                    self.env.error(format!("while condition must be bool, got {cond_ty}"), w.condition.span());
                }
                self.check_block(&w.body);
            }
            Stmt::For(f) => {
                self.env.push_scope();
                if let Some(init) = &f.init { self.check_stmt(init); }
                if let Some(cond) = &f.condition { self.check_expr(cond); }
                if let Some(upd) = &f.update { self.check_stmt(upd); }
                self.check_block(&f.body);
                self.env.pop_scope();
            }
            Stmt::Return(r) => {
                if let Some(val) = &r.value { self.check_expr(val); }
            }
            Stmt::Break(_) | Stmt::Continue(_) => {}
            Stmt::ThreadSpawn(ts) => self.check_thread_spawn(ts),
            Stmt::ScopeBlock(sb) => {
                for arg in &sb.args { self.check_expr(arg); }
                // Collect template params + define fields before mutably borrowing env.
                let scope_bindings: Vec<(String, Type)> = match &sb.template {
                    ScopeTemplateRef::Named(ident) => {
                        if let Some((_id, info)) = self.env.lookup_template(&ident.name) {
                            info.params.iter()
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .chain(info.define_fields.iter().map(|(k, v)| (k.clone(), v.clone())))
                                .collect()
                        } else {
                            vec![]
                        }
                    }
                    ScopeTemplateRef::Anonymous(decl) => {
                        let mut b: Vec<(String, Type)> = decl.params.iter()
                            .map(|p| (p.name.name.clone(), self.resolve_type(&p.ty)))
                            .collect();
                        for m in &decl.members {
                            if let ScopeTemplateMember::Define(e) = m {
                                b.push((e.name.name.clone(), self.resolve_type(&e.ty)));
                            }
                        }
                        b
                    }
                };
                // Push scope and inject bindings so the body can see params + define fields.
                self.env.push_scope();
                for (name, ty) in scope_bindings {
                    self.env.define(name, ty);
                }
                for s in &sb.body.stmts { self.check_stmt(s); }
                self.env.pop_scope();
            }
            Stmt::ExclusiveBlock(eb) => {
                let old = self.env.in_exclusive;
                self.env.in_exclusive = true;
                self.check_block(&eb.body);
                self.env.in_exclusive = old;
            }
            Stmt::Expr(es) => { self.check_expr(&es.expr); }
        }
    }

    fn check_block(&mut self, b: &Block) {
        self.env.push_scope();
        for s in &b.stmts { self.check_stmt(s); }
        self.env.pop_scope();
    }

    fn check_thread_spawn(&mut self, ts: &ThreadSpawnStmt) {
        for arg in &ts.args { self.check_expr(arg); }
        // Register handle type if we know the template
        if let HandleBind::Bind(name) = &ts.handle_bind {
            let handle_ty = match &ts.template {
                ThreadTemplateRef::Named(n) => {
                    if let Some((id, _)) = self.env.lookup_template(&n.name) {
                        Type::ThreadHandle(id)
                    } else {
                        self.env.error(format!("unknown thread template '{}'", n.name), n.span);
                        Type::Error
                    }
                }
                _ => Type::Error,
            };
            self.env.define(name.name.clone(), handle_ty);
        }
        let prev_func = std::mem::replace(
            &mut self.env.current_func,
            FuncContext::AsyncFunction { return_type: Type::Void },
        );
        self.check_block(&ts.body);
        self.env.current_func = prev_func;
    }

    // ── Expressions ───────────────────────────────────────────────────────────

    pub fn check_expr(&mut self, e: &Expr) -> Type {
        match e {
            Expr::Lit(l) => self.check_literal(l),
            Expr::Ident(i) => {
                self.env.lookup(&i.name).cloned().unwrap_or_else(|| {
                    self.env.error(format!("undefined variable '{}'", i.name), i.span);
                    Type::Error
                })
            }
            Expr::BinOp(b) => self.check_binop(b),
            Expr::UnaryOp(u) => self.check_unary(u),
            Expr::Call(c) => {
                for arg in &c.args { self.check_expr(arg); }
                // Recognize well-known builtins with meaningful return types.
                if let Expr::Ident(i) = &c.callee {
                    match i.name.as_str() {
                        "keepalive" => return Type::Never,
                        "getchar"   => return Type::Option(Box::new(Type::Char)),
                        "print" | "println" | "asleep" => return Type::Void,
                        _ => {}
                    }
                }
                Type::Error
            }
            Expr::MethodCall(m) => self.check_method_call(m),
            Expr::FieldAccess(f) => self.check_field_access(f),
            Expr::Index(i) => {
                let obj_ty = self.check_expr(&i.object);
                self.check_expr(&i.index);
                match obj_ty {
                    Type::List(inner) => *inner,
                    _ => Type::Error,
                }
            }
            Expr::Await(a) => {
                let inner = self.check_expr(&a.expr);
                match inner {
                    Type::Future(t) => *t,
                    _ => {
                        if !matches!(self.env.current_func, FuncContext::AsyncFunction { .. }) {
                            self.env.error("await can only be used in async functions", a.span);
                        }
                        Type::Error
                    }
                }
            }
            Expr::Panic(_) => Type::Never,
            Expr::Assert(_) => Type::Void,
            Expr::TypeCtor(tc) => self.check_type_ctor(tc),
        }
    }

    fn check_literal(&self, l: &Literal) -> Type {
        match &l.kind {
            LitKind::Bool(_) => Type::Bool,
            LitKind::Int(_) => Type::Int,
            LitKind::Double(_) => Type::Double,
            LitKind::Char(_) => Type::Char,
            LitKind::String(_) => Type::TString,
            LitKind::None => {
                if let Some(Type::Option(inner)) = &self.expected_ty {
                    Type::Option(inner.clone())
                } else {
                    Type::Option(Box::new(Type::Error))
                }
            }
        }
    }

    fn check_binop(&mut self, b: &BinOpExpr) -> Type {
        let lty = self.check_expr(&b.left);
        let rty = self.check_expr(&b.right);
        match b.op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
                if lty == rty && lty.is_numeric() { lty }
                else { Type::Error }
            }
            BinOp::Eq | BinOp::Ne => Type::Bool,
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                if lty == rty && lty.is_numeric() { Type::Bool }
                else { Type::Error }
            }
            BinOp::And | BinOp::Or => {
                if lty == Type::Bool && rty == Type::Bool { Type::Bool }
                else {
                    self.env.error("logical operators require bool operands", b.span);
                    Type::Error
                }
            }
        }
    }

    fn check_unary(&mut self, u: &UnaryOpExpr) -> Type {
        let inner = self.check_expr(&u.operand);
        match u.op {
            UnaryOp::Neg => {
                if inner.is_numeric() { inner }
                else { self.env.error("negation requires numeric type", u.span); Type::Error }
            }
            UnaryOp::Not => {
                if inner == Type::Bool { Type::Bool }
                else { self.env.error("logical not requires bool", u.span); Type::Error }
            }
        }
    }

    fn check_method_call(&mut self, m: &MethodCallExpr) -> Type {
        let recv_ty = self.check_expr(&m.receiver);
        for arg in &m.args { self.check_expr(arg); }
        match m.method.name.as_str() {
            "wait" => {
                match recv_ty {
                    Type::Future(inner) => *inner,
                    _ => Type::Error,
                }
            }
            "waitHandler" | "awaitHandler" => {
                match recv_ty {
                    Type::HandlerFuture(inner) => {
                        // Returns Result<inner, HandlerDispatchError>
                        Type::Result(inner, Box::new(Type::Error))
                    }
                    _ => Type::Error,
                }
            }
            "terminate" => Type::Future(Box::new(Type::Void)),
            "length" | "size" => Type::Int,
            "push" | "pop" | "set" | "remove" | "close" => Type::Void,
            "get" => {
                match recv_ty {
                    Type::List(inner) => *inner,
                    Type::Map(_, v) => *v,
                    _ => Type::Error,
                }
            }
            "isSome" | "isNone" | "isOk" | "isErr" => Type::Bool,
            "unwrap" => {
                match recv_ty {
                    Type::Option(inner) => *inner,
                    Type::Result(ok, _) => *ok,
                    _ => Type::Error,
                }
            }
            "unwrapErr" => {
                match recv_ty {
                    Type::Result(_, err) => *err,
                    _ => Type::Error,
                }
            }
            "unwrapOr" => {
                match recv_ty {
                    Type::Option(inner) | Type::Result(inner, _) => *inner,
                    _ => Type::Error,
                }
            }
            _ => {
                // Could be a handler call: returns HandlerFuture<R>
                if let Type::ThreadHandle(id) = recv_ty {
                    // find the template by id
                    for (_, (tid, info)) in &self.env.templates {
                        if *tid == id {
                            if let Some(sig) = info.handlers.get(&m.method.name) {
                                return Type::HandlerFuture(Box::new(sig.return_type.clone()));
                            }
                        }
                    }
                }
                Type::Error
            }
        }
    }

    fn check_field_access(&mut self, f: &FieldAccessExpr) -> Type {
        let obj_ty = self.check_expr(&f.object);
        if let Type::ThreadHandle(id) = obj_ty {
            for (_, (tid, info)) in &self.env.templates {
                if *tid == id {
                    if let Some(ei) = info.expose_fields.get(&f.field.name) {
                        return ei.ty.clone();
                    }
                }
            }
        }
        Type::Error
    }

    fn check_type_ctor(&mut self, tc: &TypeCtorExpr) -> Type {
        for arg in &tc.args { self.check_expr(arg); }
        match tc.name.as_str() {
            "Ok" => {
                let inner = tc.args.first().map(|a| self.check_expr(a)).unwrap_or(Type::Error);
                let err = if let Some(Type::Result(_, e)) = &self.expected_ty { *e.clone() } else { Type::Error };
                Type::Result(Box::new(inner), Box::new(err))
            }
            "Err" => {
                let inner = tc.args.first().map(|a| self.check_expr(a)).unwrap_or(Type::Error);
                let ok = if let Some(Type::Result(t, _)) = &self.expected_ty { *t.clone() } else { Type::Error };
                Type::Result(Box::new(ok), Box::new(inner))
            }
            "Some" => {
                let inner = tc.args.first().map(|a| self.check_expr(a)).unwrap_or(Type::Error);
                Type::Option(Box::new(inner))
            }
            "None" => {
                if let Some(Type::Option(inner)) = &self.expected_ty {
                    Type::Option(inner.clone())
                } else {
                    Type::Option(Box::new(Type::Error))
                }
            }
            "List" => {
                let elem = tc.type_args.first().map(|t| self.resolve_type(t)).unwrap_or(Type::Error);
                Type::List(Box::new(elem))
            }
            "Map" => {
                let k = tc.type_args.first().map(|t| self.resolve_type(t)).unwrap_or(Type::Error);
                let v = tc.type_args.get(1).map(|t| self.resolve_type(t)).unwrap_or(Type::Error);
                Type::Map(Box::new(k), Box::new(v))
            }
            "locked" => {
                let inner = tc.type_args.first().map(|t| self.resolve_type(t)).unwrap_or(Type::Error);
                Type::Locked(Box::new(inner))
            }
            "Queue" => {
                let inner = tc.type_args.first().map(|t| self.resolve_type(t)).unwrap_or(Type::Error);
                Type::Queue(Box::new(inner))
            }
            _ => Type::Error,
        }
    }
}
