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
        // Pass 1: register all template names with empty placeholder info so that
        // forward references (e.g. thread<worker> used before worker is declared)
        // are visible during type resolution in pass 2.
        for item in &prog.items {
            match item {
                TopLevelItem::ScopeTemplateDecl(d) => {
                    let name = d.name.as_ref().map(|i| i.name.clone()).unwrap_or_default();
                    if !name.is_empty() {
                        self.env.register_template(name, TemplateInfo {
                            kind: TemplateKind::Scope,
                            params: vec![],
                            define_fields: IndexMap::new(),
                            expose_fields: IndexMap::new(),
                            handlers: IndexMap::new(),
                            is_terminatable: false,
                        });
                    }
                }
                TopLevelItem::ThreadTemplateDecl(d) => {
                    let name = d.name.as_ref().map(|i| i.name.clone()).unwrap_or_default();
                    if !name.is_empty() {
                        self.env.register_template(name, TemplateInfo {
                            kind: TemplateKind::Thread,
                            params: vec![],
                            define_fields: IndexMap::new(),
                            expose_fields: IndexMap::new(),
                            handlers: IndexMap::new(),
                            is_terminatable: false,
                        });
                    }
                }
                TopLevelItem::FuncDef(_) | TopLevelItem::Statement(_) => {}
            }
        }
        // Pass 2: resolve types and fill in the full template info
        for item in &prog.items {
            match item {
                TopLevelItem::ScopeTemplateDecl(d) => self.register_scope_template(d),
                TopLevelItem::ThreadTemplateDecl(d) => self.register_thread_template(d),
                TopLevelItem::FuncDef(_) | TopLevelItem::Statement(_) => {}
            }
        }
        // Pass 3: type-check template member bodies
        for item in &prog.items {
            match item {
                TopLevelItem::ScopeTemplateDecl(d) => self.check_scope_template_bodies(d),
                TopLevelItem::ThreadTemplateDecl(d) => self.check_thread_template_bodies(d),
                TopLevelItem::FuncDef(_) | TopLevelItem::Statement(_) => {}
            }
        }
        // Pass 4: type-check top-level statements
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
        self.env.update_template(&name, info);
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
        self.env.update_template(&name, info);
    }

    // ── Template body checking ────────────────────────────────────────────────

    fn check_scope_template_bodies(&mut self, d: &ScopeTemplateDecl) {
        let mut bindings: Vec<(String, Type)> = d.params.iter()
            .map(|p| (p.name.name.clone(), self.resolve_type(&p.ty)))
            .collect();
        for m in &d.members {
            if let ScopeTemplateMember::Define(e) = m {
                bindings.push((e.name.name.clone(), self.resolve_type(&e.ty)));
            }
        }
        for m in &d.members {
            let func = match m {
                ScopeTemplateMember::OnEnter(f) | ScopeTemplateMember::OnExit(f) | ScopeTemplateMember::MemberFunc(f) => f,
                ScopeTemplateMember::Define(_) => continue,
            };
            self.check_func_body(func, &bindings);
        }
    }

    fn check_thread_template_bodies(&mut self, d: &ThreadTemplateDecl) {
        let mut bindings: Vec<(String, Type)> = d.params.iter()
            .map(|p| (p.name.name.clone(), self.resolve_type(&p.ty)))
            .collect();
        for m in &d.members {
            match m {
                ThreadTemplateMember::Expose(e) | ThreadTemplateMember::ExposeMutable(e) | ThreadTemplateMember::Define(e) => {
                    bindings.push((e.name.name.clone(), self.resolve_type(&e.ty)));
                }
                _ => {}
            }
        }

        for m in &d.members {
            match m {
                ThreadTemplateMember::OnEnter(f) | ThreadTemplateMember::OnExit(f)
                | ThreadTemplateMember::OnTerminate(f) | ThreadTemplateMember::MemberFunc(f) => {
                    self.check_func_body(f, &bindings);
                }
                ThreadTemplateMember::Handler(h) => {
                    self.check_handler_body(h, &bindings);
                }
                _ => {}
            }
        }
    }

    fn check_func_body(&mut self, f: &FuncDef, extra_bindings: &[(String, Type)]) {
        let ret = self.resolve_type(&f.return_type);
        let ctx = if f.kind == tessera_ast::FuncKind::Async {
            FuncContext::AsyncFunction { return_type: ret }
        } else {
            FuncContext::SyncFunction { return_type: ret }
        };
        let prev = std::mem::replace(&mut self.env.current_func, ctx);
        self.env.push_scope();
        for (name, ty) in extra_bindings { self.env.define(name.clone(), ty.clone()); }
        for p in &f.params { let ty = self.resolve_type(&p.ty); self.env.define(p.name.name.clone(), ty); }
        self.check_block(&f.body);
        self.env.pop_scope();
        self.env.current_func = prev;
    }

    fn check_handler_body(&mut self, h: &HandlerDef, extra_bindings: &[(String, Type)]) {
        let ret = self.resolve_type(&h.return_type);
        let prev = std::mem::replace(&mut self.env.current_func, FuncContext::Handler { return_type: ret });
        self.env.push_scope();
        for (name, ty) in extra_bindings { self.env.define(name.clone(), ty.clone()); }
        for p in &h.params { let ty = self.resolve_type(&p.ty); self.env.define(p.name.name.clone(), ty); }
        self.check_block(&h.body);
        self.env.pop_scope();
        self.env.current_func = prev;
    }

    // ── Type resolution ───────────────────────────────────────────────────────

    pub fn resolve_type(&mut self, te: &TypeExpr) -> Type {
        match te {
            TypeExpr::Void => Type::Void,
            TypeExpr::Never => Type::Never,
            TypeExpr::Named(ident, args) => self.resolve_named_type(&ident.name, args, ident.span),
        }
    }

    fn resolve_named_type(&mut self, name: &str, args: &[TypeExpr], span: Span) -> Type {
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
            "signal" => Type::Signal,
            "contract" => Type::Contract,
            "HandlerDispatchError" => Type::HandlerDispatchError,
            "thread" => {
                // thread<TemplateName> — the type arg names the template
                if let Some(TypeExpr::Named(ident, _)) = args.first() {
                    if let Some((id, _)) = self.env.lookup_template(&ident.name) {
                        return Type::ThreadHandle(id);
                    }
                    self.env.error(format!("unknown thread template '{}'", ident.name), ident.span);
                } else {
                    self.env.error("thread<> requires a template name argument".to_string(), span);
                }
                Type::Error
            }
            other => {
                // Bare template name used directly as a type (legacy / shorthand)
                if let Some((id, _)) = self.env.lookup_template(other) {
                    Type::ThreadHandle(id)
                } else {
                    self.env.error(format!("unknown type '{other}'"), span);
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
                        "signal"   => return Type::Signal,
                        "contract" => return Type::Contract,
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
                    // `await s` on a signal/contract resolves to void (spec §11.3.5 / §12.4.4)
                    Type::Signal | Type::Contract => Type::Void,
                    _ => {
                        if !matches!(self.env.current_func, FuncContext::AsyncFunction { .. } | FuncContext::Handler { .. }) {
                            self.env.error("await can only be used in async functions or handlers", a.span);
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
                    Type::Signal | Type::Contract => Type::Void,
                    _ => Type::Error,
                }
            }
            // signal methods
            "raise" | "reset" => Type::Void,
            "isRaised" => Type::Bool,
            "awaitSignal" => Type::Void,
            // contract methods
            "fulfill" => Type::Void,
            "isPending" => Type::Bool,
            "awaitContract" => Type::Void,
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
            "enqueue" => Type::Void,
            "dequeue" => {
                match recv_ty {
                    Type::Queue(inner) => Type::Option(inner),
                    _ => Type::Error,
                }
            }
            "isEmpty" => Type::Bool,
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
                    self.env.error(format!("unknown handler '{}' on thread handle", m.method.name), m.method.span);
                } else if recv_ty != Type::Error {
                    self.env.error(format!("unknown method '{}' on type {recv_ty}", m.method.name), m.method.span);
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
            self.env.error(format!("unknown field '{}' on thread handle", f.field.name), f.field.span);
        } else if obj_ty != Type::Error {
            self.env.error(format!("field access on non-thread type {obj_ty}", ), f.field.span);
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
