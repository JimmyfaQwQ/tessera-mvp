use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use tokio::sync::{mpsc, Mutex as AsyncMutex};

/// A single background OS thread reads stdin and sends chars into this channel.
/// Using std::thread::spawn (not spawn_blocking) means tokio does not wait for
/// it on shutdown, so the process can exit cleanly without an extra keypress.
fn stdin_receiver() -> &'static AsyncMutex<mpsc::Receiver<Option<char>>> {
    static INSTANCE: OnceLock<AsyncMutex<mpsc::Receiver<Option<char>>>> = OnceLock::new();
    INSTANCE.get_or_init(|| {
        let (tx, rx) = mpsc::channel(256);
        std::thread::spawn(move || {
            use std::io::Read;
            let stdin = std::io::stdin();
            let mut handle = stdin.lock();
            let mut first = [0u8; 1];
            loop {
                match handle.read(&mut first) {
                    Ok(0) | Err(_) => { let _ = tx.blocking_send(None); break; }
                    Ok(_) => {
                        let byte = first[0];
                        let seq_len = if byte < 0x80 { 1 }
                                      else if byte < 0xE0 { 2 }
                                      else if byte < 0xF0 { 3 }
                                      else { 4 };
                        let mut buf = [0u8; 4];
                        buf[0] = byte;
                        if seq_len > 1 && handle.read_exact(&mut buf[1..seq_len]).is_err() {
                            let _ = tx.blocking_send(None);
                            break;
                        }
                        let ch = std::str::from_utf8(&buf[..seq_len])
                            .ok()
                            .and_then(|s| s.chars().next());
                        if tx.blocking_send(ch).is_err() { break; }
                    }
                }
            }
        });
        AsyncMutex::new(rx)
    })
}

use async_recursion::async_recursion;
use tessera_ast::*;
use tessera_runtime::{
    Value, RuntimeError,
    TesseraLocked, TesseraQueue, FutureOutcome, ThreadState,
    HandlerRequest,
};
use crate::env::Env;

// ── Interpreter state (shared via Rc between main body task + handler tasks) ──

pub struct InterpState {
    pub env: RefCell<Env>,
    pub func_table: RefCell<HashMap<String, Arc<FuncDef>>>,
    pub scope_templates: RefCell<HashMap<String, Arc<ScopeTemplateDecl>>>,
    pub thread_templates: RefCell<HashMap<String, Arc<ThreadTemplateDecl>>>,
    pub current_thread_state: RefCell<Option<Arc<ThreadState>>>,
    pub expose_field_names: RefCell<HashSet<String>>,
    pub expose_mutable_field_names: RefCell<HashSet<String>>,
}

impl InterpState {
    pub fn new() -> Self {
        Self {
            env: RefCell::new(Env::new()),
            func_table: RefCell::new(HashMap::new()),
            scope_templates: RefCell::new(HashMap::new()),
            thread_templates: RefCell::new(HashMap::new()),
            current_thread_state: RefCell::new(None),
            expose_field_names: RefCell::new(HashSet::new()),
            expose_mutable_field_names: RefCell::new(HashSet::new()),
        }
    }

    /// Create a child state for a spawned thread: shares template tables, fresh env.
    pub fn new_for_thread(parent: &Rc<InterpState>) -> Rc<InterpState> {
        Rc::new(InterpState {
            env: RefCell::new(Env::new()),
            func_table: RefCell::new(parent.func_table.borrow().clone()),
            scope_templates: RefCell::new(parent.scope_templates.borrow().clone()),
            thread_templates: RefCell::new(parent.thread_templates.borrow().clone()),
            current_thread_state: RefCell::new(None),
            expose_field_names: RefCell::new(HashSet::new()),
            expose_mutable_field_names: RefCell::new(HashSet::new()),
        })
    }
}

// ── Interpreter ───────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct Interpreter(pub Rc<InterpState>);

impl Interpreter {
    pub fn new() -> Self {
        Self(Rc::new(InterpState::new()))
    }

    pub fn new_for_thread(parent: &Interpreter) -> Self {
        Self(InterpState::new_for_thread(&parent.0))
    }

    // ── Program entry ─────────────────────────────────────────────────────────

    pub async fn run_program(&self, prog: &Program) -> Result<(), RuntimeError> {
        // Register all templates and functions first
        for item in &prog.items {
            match item {
                TopLevelItem::ScopeTemplateDecl(d) => {
                    if let Some(name) = &d.name {
                        self.0.scope_templates.borrow_mut()
                            .insert(name.name.clone(), Arc::new(d.clone()));
                    }
                }
                TopLevelItem::ThreadTemplateDecl(d) => {
                    if let Some(name) = &d.name {
                        self.0.thread_templates.borrow_mut()
                            .insert(name.name.clone(), Arc::new(d.clone()));
                    }
                }
                TopLevelItem::FuncDef(f) => {
                    self.0.func_table.borrow_mut()
                        .insert(f.name.name.clone(), Arc::new(f.clone()));
                }
                TopLevelItem::Statement(_) => {}
            }
        }
        // Execute top-level statements
        for item in &prog.items {
            if let TopLevelItem::Statement(s) = item {
                self.exec_stmt(s).await?;
            }
        }
        Ok(())
    }

    // ── Statements ────────────────────────────────────────────────────────────

    #[async_recursion(?Send)]
    pub async fn exec_stmt(&self, s: &Stmt) -> Result<Option<Value>, RuntimeError> {
        match s {
            Stmt::Let(l) => {
                let v = self.eval_expr(&l.init).await?;
                let name = l.name.name.clone();
                self.maybe_sync_expose(&name, &v);
                self.0.env.borrow_mut().define(name, v);
                Ok(None)
            }
            Stmt::Assign(a) => {
                let v = self.eval_expr(&a.value).await?;
                match &a.target {
                    AssignTarget::Ident(i) => {
                        self.maybe_sync_expose(&i.name, &v);
                        if !self.0.env.borrow_mut().assign(&i.name, v) {
                            return Err(RuntimeError::UndefinedVariable {
                                name: i.name.clone(),
                                location: i.span,
                            });
                        }
                    }
                    AssignTarget::Field(obj_expr, field) => {
                        let obj = self.eval_expr(obj_expr).await?;
                        if let Value::ThreadHandle(state) = obj {
                            // Only allow writing if it is NOT an expose_mutable field —
                            // expose_mutable exposes the value for mutation via its own
                            // methods (.get()/.set()), but the field reference itself
                            // cannot be replaced from outside.
                            if state.expose_mutable_fields.read().await.contains_key(&field.name) {
                                return Err(RuntimeError::ExposeMutableFieldReplace {
                                    location: a.span,
                                });
                            }
                        }
                    }
                    AssignTarget::Index(obj_expr, idx_expr) => {
                        let obj = self.eval_expr(obj_expr).await?;
                        let idx = self.eval_expr(idx_expr).await?;
                        if let Value::List(list) = obj {
                            if let Value::Int(i) = idx {
                                let mut l = list.borrow_mut();
                                let len = l.len();
                                let ui = i as usize;
                                if ui >= len {
                                    return Err(RuntimeError::IndexOutOfBounds {
                                        index: i,
                                        length: len as i32,
                                        location: a.span,
                                    });
                                }
                                l[ui] = v;
                            }
                        }
                    }
                }
                Ok(None)
            }
            Stmt::If(i) => {
                let cond = self.eval_expr(&i.condition).await?;
                if truthy(cond) {
                    return self.exec_block(&i.then_block).await;
                }
                if let Some(eb) = &i.else_branch {
                    match eb {
                        ElseBranch::Else(b) => return self.exec_block(b).await,
                        ElseBranch::ElseIf(s) => return self.exec_stmt(&Stmt::If(*s.clone())).await,
                    }
                }
                Ok(None)
            }
            Stmt::While(w) => {
                loop {
                    let cond = self.eval_expr(&w.condition).await?;
                    if !truthy(cond) { break; }
                    match self.exec_block(&w.body).await? {
                        Some(Value::Never) => break, // break statement
                        Some(v) => return Ok(Some(v)),
                        None => {}
                    }
                }
                Ok(None)
            }
            Stmt::For(f) => {
                self.0.env.borrow_mut().push_scope();
                if let Some(init) = &f.init {
                    self.exec_stmt(init).await?;
                }
                loop {
                    if let Some(cond) = &f.condition {
                        let v = self.eval_expr(cond).await?;
                        if !truthy(v) { break; }
                    }
                    match self.exec_block(&f.body).await? {
                        Some(Value::Never) => break, // break
                        Some(v) => {
                            self.0.env.borrow_mut().pop_scope();
                            return Ok(Some(v));
                        }
                        None => {}
                    }
                    if let Some(upd) = &f.update {
                        self.exec_stmt(upd).await?;
                    }
                }
                self.0.env.borrow_mut().pop_scope();
                Ok(None)
            }
            Stmt::Return(r) => {
                let v = if let Some(e) = &r.value {
                    self.eval_expr(e).await?
                } else {
                    Value::Void
                };
                Ok(Some(v))
            }
            Stmt::Break(_) => Ok(Some(Value::Never)),
            Stmt::Continue(_) => Ok(None),

            Stmt::ExclusiveBlock(eb) => {
                if let Some(state) = self.0.current_thread_state.borrow().clone() {
                    state.set_exclusive(true);
                }
                let result = self.exec_block(&eb.body).await;
                if let Some(state) = self.0.current_thread_state.borrow().clone() {
                    state.set_exclusive(false);
                }
                result
            }

            Stmt::ThreadSpawn(ts) => self.exec_thread_spawn(ts).await,

            Stmt::ScopeBlock(sb) => self.exec_scope_block(sb).await,

            Stmt::Expr(es) => {
                self.eval_expr(&es.expr).await?;
                Ok(None)
            }
        }
    }

    #[async_recursion(?Send)]
    pub async fn exec_block(&self, b: &Block) -> Result<Option<Value>, RuntimeError> {
        self.0.env.borrow_mut().push_scope();
        for s in &b.stmts {
            match self.exec_stmt(s).await? {
                Some(v) => {
                    self.0.env.borrow_mut().pop_scope();
                    return Ok(Some(v));
                }
                None => {}
            }
        }
        self.0.env.borrow_mut().pop_scope();
        Ok(None)
    }

    // ── Scope template execution (Gap 2) ──────────────────────────────────────

    async fn exec_scope_block(&self, sb: &ScopeBlockStmt) -> Result<Option<Value>, RuntimeError> {
        let decl = match &sb.template {
            ScopeTemplateRef::Named(n) => {
                self.0.scope_templates.borrow().get(&n.name).cloned()
            }
            ScopeTemplateRef::Anonymous(d) => Some(Arc::new(*d.clone())),
        };

        // Bind args to params in a new scope
        self.0.env.borrow_mut().push_scope();
        if let Some(decl) = &decl {
            for (param, arg_expr) in decl.params.iter().zip(sb.args.iter()) {
                let v = self.eval_expr(arg_expr).await?;
                self.0.env.borrow_mut().define(param.name.name.clone(), v);
            }
            // Initialize define fields before __on_enter__
            for m in &decl.members {
                if let ScopeTemplateMember::Define(e) = m {
                    let val = if let Some(init) = &e.initializer {
                        self.eval_expr(init).await.unwrap_or_else(|_| crate::event_loop::default_value_for_type(&e.ty))
                    } else {
                        crate::event_loop::default_value_for_type(&e.ty)
                    };
                    self.0.env.borrow_mut().define(e.name.name.clone(), val);
                }
            }
            // Run __on_enter__
            if let Some(on_enter) = find_scope_hook(decl, "__on_enter__") {
                self.exec_block(&on_enter.body).await?;
            }
        }

        // Run user body (save error for after on_exit)
        let body_result = self.exec_block(&sb.body).await;

        // Run __on_exit__ unconditionally
        if let Some(decl) = &decl {
            if let Some(on_exit) = find_scope_hook(decl, "__on_exit__") {
                let _ = self.exec_block(&on_exit.body).await;
            }
        }

        self.0.env.borrow_mut().pop_scope();

        // Re-propagate body error (after on_exit ran)
        body_result
    }

    // ── Thread spawning (Gap 3b) ───────────────────────────────────────────────

    async fn exec_thread_spawn(&self, ts: &ThreadSpawnStmt) -> Result<Option<Value>, RuntimeError> {
        use tokio::sync::mpsc;

        // Evaluate args before creating the child interpreter
        let mut arg_values = Vec::new();
        for a in &ts.args { arg_values.push(self.eval_expr(a).await?); }

        let (template_name, decl) = match &ts.template {
            ThreadTemplateRef::Named(n) => {
                let d = self.0.thread_templates.borrow().get(&n.name).cloned();
                (Some(n.name.clone()), d)
            }
            ThreadTemplateRef::Anonymous(d) => {
                (d.name.as_ref().map(|n| n.name.clone()), Some(Arc::new(*d.clone())))
            }
            ThreadTemplateRef::Shorthand => (None, None),
        };

        let (handler_tx, handler_rx) = mpsc::channel::<HandlerRequest>(64);
        let thread_state = ThreadState::new(template_name, handler_tx);

        let child_state = InterpState::new_for_thread(&self.0);
        let child_interp = Interpreter(child_state);
        let body = Arc::new(ts.body.clone());
        let state_clone = thread_state.clone();

        tokio::task::spawn_local(crate::event_loop::run_thread_task(
            child_interp,
            decl,
            arg_values,
            body,
            state_clone,
            handler_rx,
        ));

        // Yield to let the spawned thread run __on_enter__ before the parent continues.
        tokio::task::yield_now().await;

        if let HandleBind::Bind(name) = &ts.handle_bind {
            self.0.env.borrow_mut().define(
                name.name.clone(),
                Value::ThreadHandle(thread_state),
            );
        }

        Ok(None)
    }

    // ── Expressions ───────────────────────────────────────────────────────────

    #[async_recursion(?Send)]
    pub async fn eval_expr(&self, e: &Expr) -> Result<Value, RuntimeError> {
        match e {
            Expr::Lit(l) => Ok(eval_literal(l)),
            Expr::Ident(i) => {
                let val = self.0.env.borrow().lookup(&i.name).cloned();
                val.ok_or_else(|| RuntimeError::UndefinedVariable {
                    name: i.name.clone(),
                    location: i.span,
                })
            }
            Expr::BinOp(b) => self.eval_binop(b).await,
            Expr::UnaryOp(u) => self.eval_unary(u).await,
            Expr::Call(c) => self.eval_call(c).await,
            Expr::MethodCall(m) => self.eval_method_call(m).await,
            Expr::FieldAccess(f) => self.eval_field_access(f).await,
            Expr::Index(i) => self.eval_index(i).await,
            Expr::Await(a) => self.eval_await(a).await,
            Expr::Panic(p) => {
                let msg = self.eval_expr(&p.message).await?;
                Err(RuntimeError::Panic { message: value_to_string(&msg), location: p.span })
            }
            Expr::Assert(a) => {
                let cond = self.eval_expr(&a.condition).await?;
                if !truthy(cond) {
                    let msg = if let Some(m) = &a.message {
                        value_to_string(&self.eval_expr(m).await?)
                    } else {
                        "assertion failed".into()
                    };
                    return Err(RuntimeError::AssertionFailed { message: msg, location: a.span });
                }
                Ok(Value::Void)
            }
            Expr::TypeCtor(tc) => self.eval_type_ctor(tc).await,
        }
    }

    #[async_recursion(?Send)]
    async fn eval_binop(&self, b: &BinOpExpr) -> Result<Value, RuntimeError> {
        let lv = self.eval_expr(&b.left).await?;
        match b.op {
            BinOp::And => {
                if !truthy(lv) { return Ok(Value::Bool(false)); }
                let rv = self.eval_expr(&b.right).await?;
                return Ok(Value::Bool(truthy(rv)));
            }
            BinOp::Or => {
                if truthy(lv.clone()) { return Ok(Value::Bool(true)); }
                let rv = self.eval_expr(&b.right).await?;
                return Ok(Value::Bool(truthy(rv)));
            }
            _ => {}
        }
        let rv = self.eval_expr(&b.right).await?;
        match (&b.op, &lv, &rv) {
            (BinOp::Add, Value::Int(a), Value::Int(r)) => Ok(Value::Int(a.wrapping_add(*r))),
            (BinOp::Sub, Value::Int(a), Value::Int(r)) => Ok(Value::Int(a.wrapping_sub(*r))),
            (BinOp::Mul, Value::Int(a), Value::Int(r)) => Ok(Value::Int(a.wrapping_mul(*r))),
            (BinOp::Div, Value::Int(a), Value::Int(r)) => {
                if *r == 0 { return Err(RuntimeError::DivisionByZero { location: b.span }); }
                Ok(Value::Int(a / r))
            }
            (BinOp::Rem, Value::Int(a), Value::Int(r)) => {
                if *r == 0 { return Err(RuntimeError::DivisionByZero { location: b.span }); }
                Ok(Value::Int(a % r))
            }
            (BinOp::Add, Value::Double(a), Value::Double(r)) => Ok(Value::Double(a + r)),
            (BinOp::Sub, Value::Double(a), Value::Double(r)) => Ok(Value::Double(a - r)),
            (BinOp::Mul, Value::Double(a), Value::Double(r)) => Ok(Value::Double(a * r)),
            (BinOp::Div, Value::Double(a), Value::Double(r)) => Ok(Value::Double(a / r)),
            (BinOp::Add, Value::Str(a), r) => Ok(Value::Str(format!("{a}{}", value_to_string(r)))),
            (BinOp::Add, l, Value::Str(r)) => Ok(Value::Str(format!("{}{r}", value_to_string(l)))),
            (BinOp::Eq, a, r) => Ok(Value::Bool(values_equal(a, r))),
            (BinOp::Ne, a, r) => Ok(Value::Bool(!values_equal(a, r))),
            (BinOp::Lt, Value::Int(a), Value::Int(r)) => Ok(Value::Bool(a < r)),
            (BinOp::Le, Value::Int(a), Value::Int(r)) => Ok(Value::Bool(a <= r)),
            (BinOp::Gt, Value::Int(a), Value::Int(r)) => Ok(Value::Bool(a > r)),
            (BinOp::Ge, Value::Int(a), Value::Int(r)) => Ok(Value::Bool(a >= r)),
            (BinOp::Lt, Value::Double(a), Value::Double(r)) => Ok(Value::Bool(a < r)),
            (BinOp::Le, Value::Double(a), Value::Double(r)) => Ok(Value::Bool(a <= r)),
            (BinOp::Gt, Value::Double(a), Value::Double(r)) => Ok(Value::Bool(a > r)),
            (BinOp::Ge, Value::Double(a), Value::Double(r)) => Ok(Value::Bool(a >= r)),
            _ => Err(RuntimeError::TypeMismatch {
                expected: "compatible operands".into(),
                got: format!("{} and {}", lv.type_name(), rv.type_name()),
                location: b.span,
            }),
        }
    }

    #[async_recursion(?Send)]
    async fn eval_unary(&self, u: &UnaryOpExpr) -> Result<Value, RuntimeError> {
        let v = self.eval_expr(&u.operand).await?;
        match (u.op.clone(), &v) {
            (UnaryOp::Neg, Value::Int(i)) => Ok(Value::Int(-i)),
            (UnaryOp::Neg, Value::Double(f)) => Ok(Value::Double(-f)),
            (UnaryOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
            _ => Err(RuntimeError::TypeMismatch {
                expected: "numeric or bool".into(),
                got: v.type_name().into(),
                location: u.span,
            }),
        }
    }

    // ── Function calls (Gap 1) ────────────────────────────────────────────────

    async fn eval_call(&self, c: &CallExpr) -> Result<Value, RuntimeError> {
        if let Expr::Ident(i) = &c.callee {
            match i.name.as_str() {
                "print" => {
                    for arg in &c.args {
                        let v = self.eval_expr(arg).await?;
                        print!("{}", value_to_string(&v));
                    }
                    return Ok(Value::Void);
                }
                "println" => {
                    for arg in &c.args {
                        let v = self.eval_expr(arg).await?;
                        print!("{}", value_to_string(&v));
                    }
                    println!();
                    return Ok(Value::Void);
                }
                "asleep" => {
                    let ms_val = if let Some(a) = c.args.first() {
                        self.eval_expr(a).await?
                    } else {
                        Value::Int(0)
                    };
                    let ms_u64 = match ms_val { Value::Int(n) if n > 0 => n as u64, _ => 0 };
                    if ms_u64 > 0 {
                        tokio::time::sleep(std::time::Duration::from_millis(ms_u64)).await;
                    }
                    return Ok(Value::Void);
                }
                "keepalive" => {
                    // Suspend the body future forever so the thread stays alive
                    // responding to handlers without consuming scheduler time.
                    // Silently dropped when terminate() fires; no cleanup needed.
                    std::future::pending::<()>().await;
                    return Ok(Value::Void); // unreachable
                }
                "getchar" => {
                    let mut rx = stdin_receiver().lock().await;
                    let ch = rx.recv().await.flatten();
                    return Ok(match ch {
                        Some(c) => Value::Option(Some(Box::new(Value::Char(c)))),
                        None => Value::Option(None),
                    });
                }
                name => {
                    // Look up user-defined function
                    let func = self.0.func_table.borrow().get(name).cloned();
                    if let Some(func) = func {
                        let mut args = Vec::new();
                        for a in &c.args { args.push(self.eval_expr(a).await?); }
                        return self.call_func(&func, args).await;
                    }
                }
            }
        }
        Ok(Value::Void)
    }

    pub async fn call_func(&self, func: &FuncDef, args: Vec<Value>) -> Result<Value, RuntimeError> {
        self.0.env.borrow_mut().push_scope();
        for (param, val) in func.params.iter().zip(args.into_iter()) {
            self.0.env.borrow_mut().define(param.name.name.clone(), val);
        }
        let result = self.exec_block(&func.body).await;
        self.0.env.borrow_mut().pop_scope();
        match result? {
            Some(v) => Ok(v),
            None => Ok(Value::Void),
        }
    }

    // ── Method calls ──────────────────────────────────────────────────────────

    async fn eval_method_call(&self, m: &MethodCallExpr) -> Result<Value, RuntimeError> {
        let recv = self.eval_expr(&m.receiver).await?;
        let mut args = Vec::new();
        for a in &m.args { args.push(self.eval_expr(a).await?); }

        match (&m.method.name[..], recv.clone()) {
            // Option
            ("isSome", Value::Option(v)) => Ok(Value::Bool(v.is_some())),
            ("isNone", Value::Option(v)) => Ok(Value::Bool(v.is_none())),
            ("unwrap", Value::Option(Some(v))) => Ok(*v),
            ("unwrap", Value::Option(None)) => Err(RuntimeError::UnwrapNone { location: m.span }),
            ("unwrapOr", Value::Option(Some(v))) => Ok(*v),
            ("unwrapOr", Value::Option(None)) => Ok(args.into_iter().next().unwrap_or(Value::Void)),

            // Result
            ("isOk",  Value::Result(r)) => Ok(Value::Bool(r.is_ok())),
            ("isErr", Value::Result(r)) => Ok(Value::Bool(r.is_err())),
            ("unwrap",    Value::Result(Ok(v)))  => Ok(*v),
            ("unwrap",    Value::Result(Err(_))) => Err(RuntimeError::UnwrapErr { location: m.span }),
            ("unwrapErr", Value::Result(Err(e))) => Ok(*e),
            ("unwrapOr",  Value::Result(Ok(v)))  => Ok(*v),
            ("unwrapOr",  Value::Result(Err(_))) => Ok(args.into_iter().next().unwrap_or(Value::Void)),

            // List
            ("length", Value::List(l)) => Ok(Value::Int(l.borrow().len() as i32)),
            ("push", Value::List(l)) => {
                l.borrow_mut().push(args.into_iter().next().unwrap_or(Value::Void));
                Ok(Value::Void)
            }
            ("pop", Value::List(l)) => Ok(
                l.borrow_mut().pop()
                    .map(|v| Value::Option(Some(Box::new(v))))
                    .unwrap_or(Value::Option(None))
            ),
            ("get", Value::List(l)) => {
                if let Some(Value::Int(i)) = args.first() {
                    let l = l.borrow();
                    let idx = *i as usize;
                    if idx >= l.len() {
                        return Err(RuntimeError::IndexOutOfBounds { index: *i, length: l.len() as i32, location: m.span });
                    }
                    Ok(l[idx].clone())
                } else { Ok(Value::Void) }
            }
            ("set", Value::List(l)) => {
                if let (Some(Value::Int(i)), Some(v)) = (args.first(), args.get(1)) {
                    let mut l = l.borrow_mut();
                    let idx = *i as usize;
                    if idx >= l.len() {
                        return Err(RuntimeError::IndexOutOfBounds { index: *i, length: l.len() as i32, location: m.span });
                    }
                    l[idx] = v.clone();
                }
                Ok(Value::Void)
            }

            // Future .wait()
            ("wait", Value::Future(fut)) => {
                match fut.resolve().await {
                    FutureOutcome::Ok(v) => Ok(v),
                    FutureOutcome::Failed(msg) => Err(RuntimeError::Panic { message: msg, location: m.span }),
                }
            }

            // HandlerFuture .waitHandler() / .awaitHandler()
            ("waitHandler" | "awaitHandler", Value::HandlerFuture(hf)) => {
                match hf.resolve().await {
                    Ok(v)  => Ok(Value::Result(Ok(Box::new(v)))),
                    Err(e) => Ok(Value::Result(Err(Box::new(Value::Str(e.to_string()))))),
                }
            }

            // ThreadHandle .terminate()
            ("terminate", Value::ThreadHandle(state)) => {
                let fut = state.terminate().await;
                Ok(Value::Future(fut))
            }

            // ThreadHandle handler calls: handle.methodName(args...)
            (method, Value::ThreadHandle(state)) => {
                // Dispatch handler
                let hf_result = state.dispatch_handler(method.to_string(), args).await;
                match hf_result {
                    Ok(fut) => Ok(Value::HandlerFuture(tessera_runtime::TesseraHandlerFuture::from_future(fut))),
                    Err(e)  => Ok(Value::HandlerFuture(tessera_runtime::TesseraHandlerFuture::rejected(e))),
                }
            }

            // Queue
            ("enqueue", Value::Queue(q)) => {
                let v = args.into_iter().next().unwrap_or(Value::Void);
                q.enqueue(v).await;
                Ok(Value::Void)
            }
            ("dequeue", Value::Queue(q)) => Ok(Value::Option(q.dequeue().await.map(Box::new))),
            ("tryPush",  Value::Queue(q)) => {
                let v = args.into_iter().next().unwrap_or(Value::Void);
                Ok(Value::Bool(q.try_push(v)))
            }
            ("tryPop", Value::Queue(q)) => Ok(Value::Option(q.try_pop().map(Box::new))),
            ("close",  Value::Queue(q)) => { q.close(); Ok(Value::Void) }

            // locked<T>
            ("get", Value::Locked(l)) => Ok(l.get().await),
            ("set", Value::Locked(l)) => {
                let v = args.into_iter().next().unwrap_or(Value::Void);
                l.set(v).await;
                Ok(Value::Void)
            }

            _ => Ok(Value::Void),
        }
    }

    // ── Field access (Gap 3e) ─────────────────────────────────────────────────

    async fn eval_field_access(&self, f: &FieldAccessExpr) -> Result<Value, RuntimeError> {
        let obj = self.eval_expr(&f.object).await?;
        match obj {
            Value::ThreadHandle(state) => {
                // Try expose_fields first, then expose_mutable_fields
                let val = state.expose_fields.read().await.get(&f.field.name).cloned();
                if let Some(v) = val {
                    return Ok(v);
                }
                let val = state.expose_mutable_fields.read().await.get(&f.field.name).cloned();
                Ok(val.unwrap_or(Value::Void))
            }
            _ => Ok(Value::Void),
        }
    }

    async fn eval_index(&self, i: &IndexExpr) -> Result<Value, RuntimeError> {
        let obj = self.eval_expr(&i.object).await?;
        let idx = self.eval_expr(&i.index).await?;
        match (obj, idx) {
            (Value::List(l), Value::Int(n)) => {
                let l = l.borrow();
                let ui = n as usize;
                if ui >= l.len() {
                    return Err(RuntimeError::IndexOutOfBounds { index: n, length: l.len() as i32, location: Span::dummy() });
                }
                Ok(l[ui].clone())
            }
            _ => Ok(Value::Void),
        }
    }

    async fn eval_await(&self, a: &AwaitExpr) -> Result<Value, RuntimeError> {
        let v = self.eval_expr(&a.expr).await?;
        match v {
            Value::Future(fut) => match fut.resolve().await {
                FutureOutcome::Ok(v) => Ok(v),
                FutureOutcome::Failed(msg) => Err(RuntimeError::Panic { message: msg, location: a.span }),
            },
            Value::HandlerFuture(hf) => match hf.resolve().await {
                Ok(v)  => Ok(v),
                Err(e) => Err(RuntimeError::Panic { message: e.to_string(), location: a.span }),
            },
            other => Ok(other),
        }
    }

    async fn eval_type_ctor(&self, tc: &TypeCtorExpr) -> Result<Value, RuntimeError> {
        let mut args = Vec::new();
        for a in &tc.args { args.push(self.eval_expr(a).await?); }
        match tc.name.as_str() {
            "Ok"   => Ok(Value::Result(Ok(Box::new(args.into_iter().next().unwrap_or(Value::Void))))),
            "Err"  => Ok(Value::Result(Err(Box::new(args.into_iter().next().unwrap_or(Value::Void))))),
            "Some" => Ok(Value::Option(Some(Box::new(args.into_iter().next().unwrap_or(Value::Void))))),
            "None" => Ok(Value::Option(None)),
            "List" => Ok(Value::List(Rc::new(RefCell::new(args)))),
            "locked" => {
                let initial = args.into_iter().next().unwrap_or(Value::Void);
                Ok(Value::Locked(Arc::new(TesseraLocked::new(initial))))
            }
            "Queue" => Ok(Value::Queue(Arc::new(TesseraQueue::new()))),
            _ => Ok(Value::Void),
        }
    }

    // ── Expose field sync helper ───────────────────────────────────────────────

    // Non-async: uses try_write() so the thread body never yields here.
    // try_write() always succeeds (no contention — only this task writes).
    fn maybe_sync_expose(&self, name: &str, value: &Value) {
        if self.0.expose_field_names.borrow().contains(name) {
            if let Some(state) = self.0.current_thread_state.borrow().clone() {
                if let Ok(mut guard) = state.expose_fields.try_write() {
                    guard.insert(name.to_string(), value.clone());
                }
            }
        } else if self.0.expose_mutable_field_names.borrow().contains(name) {
            if let Some(state) = self.0.current_thread_state.borrow().clone() {
                if let Ok(mut guard) = state.expose_mutable_fields.try_write() {
                    guard.insert(name.to_string(), value.clone());
                }
            }
        }
    }

    // ── Public exec helpers (used by event loop) ─────────────────────────────

    pub async fn exec_func_def_body(&self, func: &FuncDef, args: Vec<Value>) -> Result<Value, RuntimeError> {
        self.call_func(func, args).await
    }

    pub async fn exec_handler_body(&self, handler: &HandlerDef, args: Vec<Value>) -> Result<Value, RuntimeError> {
        self.0.env.borrow_mut().push_scope();
        for (param, val) in handler.params.iter().zip(args.into_iter()) {
            self.0.env.borrow_mut().define(param.name.name.clone(), val);
        }
        let result = self.exec_block(&handler.body).await;
        self.0.env.borrow_mut().pop_scope();
        match result? {
            Some(v) => Ok(v),
            None => Ok(Value::Void),
        }
    }
}

// ── Scope template helpers ────────────────────────────────────────────────────

fn find_scope_hook<'a>(decl: &'a ScopeTemplateDecl, name: &str) -> Option<&'a FuncDef> {
    for m in &decl.members {
        match m {
            ScopeTemplateMember::OnEnter(f) if name == "__on_enter__" => return Some(f),
            ScopeTemplateMember::OnExit(f)  if name == "__on_exit__"  => return Some(f),
            _ => {}
        }
    }
    None
}

pub fn find_thread_hook<'a>(decl: &'a ThreadTemplateDecl, name: &str) -> Option<&'a FuncDef> {
    for m in &decl.members {
        match m {
            ThreadTemplateMember::OnEnter(f)    if name == "__on_enter__"    => return Some(f),
            ThreadTemplateMember::OnExit(f)     if name == "__on_exit__"     => return Some(f),
            ThreadTemplateMember::OnTerminate(f) if name == "__on_terminate__" => return Some(f),
            _ => {}
        }
    }
    None
}

pub fn find_handler<'a>(decl: &'a ThreadTemplateDecl, name: &str) -> Option<&'a HandlerDef> {
    for m in &decl.members {
        if let ThreadTemplateMember::Handler(h) = m {
            if h.name.name == name { return Some(h); }
        }
    }
    None
}

// ── Literal evaluation ────────────────────────────────────────────────────────

fn eval_literal(l: &Literal) -> Value {
    match &l.kind {
        LitKind::Bool(b) => Value::Bool(*b),
        LitKind::Int(i)  => Value::Int(*i as i32),
        LitKind::Double(f) => Value::Double(*f),
        LitKind::Char(c)  => Value::Char(*c),
        LitKind::String(s) => Value::Str(s.clone()),
        LitKind::None => Value::Option(None),
    }
}

fn truthy(v: Value) -> bool {
    match v {
        Value::Bool(b)       => b,
        Value::Int(i)        => i != 0,
        Value::Option(None)  => false,
        Value::Option(Some(_)) => true,
        _ => true,
    }
}

fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Int(a),  Value::Int(b))  => a == b,
        (Value::Double(a), Value::Double(b)) => a == b,
        (Value::Char(a), Value::Char(b))  => a == b,
        (Value::Str(a),  Value::Str(b))   => a == b,
        (Value::Void,    Value::Void)     => true,
        (Value::Option(None), Value::Option(None)) => true,
        _ => false,
    }
}

pub fn value_to_string(v: &Value) -> String {
    match v {
        Value::Bool(b)   => b.to_string(),
        Value::Int(i)    => i.to_string(),
        Value::Double(f) => f.to_string(),
        Value::Char(c)   => c.to_string(),
        Value::Str(s)    => s.clone(),
        Value::Void      => "void".into(),
        Value::Never     => "never".into(),
        Value::Option(None)    => "None".into(),
        Value::Option(Some(v)) => format!("Some({})", value_to_string(v)),
        Value::Result(Ok(v))   => format!("Ok({})", value_to_string(v)),
        Value::Result(Err(e))  => format!("Err({})", value_to_string(e)),
        _ => "<complex>".into(),
    }
}
