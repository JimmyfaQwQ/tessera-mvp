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
    Value, RuntimeError, QueuePushError, HandlerDispatchError,
    TesseraLocked, TesseraQueue, TesseraSignal, TesseraContract, TesseraPermit,
    FutureOutcome, TesseraFuture, HandlerResolveResult, ThreadState,
    HandlerRequest, BreakablePrimitive, BrokenReason,
};
use tessera_runtime::value::ValueKey;
use crate::env::Env;

// ── Structured-error helpers ──────────────────────────────────────────────────

fn dispatch_error_to_kind(e: &HandlerDispatchError) -> (String, String) {
    match e {
        HandlerDispatchError::TargetTerminated  => ("TargetGone".into(),       e.to_string()),
        HandlerDispatchError::TargetTerminating => ("TargetTerminating".into(), e.to_string()),
        HandlerDispatchError::TargetCrashed     => ("TargetCrashed".into(),    e.to_string()),
    }
}

/// Convert any RuntimeError into a `Value::ErrorObj` for use in `try` results.
fn runtime_error_to_error_obj(e: &RuntimeError) -> Value {
    let (kind, message) = match e {
        RuntimeError::Panic { message, .. } =>
            ("Panic".into(), message.clone()),
        RuntimeError::AssertionFailed { message, .. } =>
            ("AssertionFailed".into(), message.clone()),
        RuntimeError::IndexOutOfBounds { index, length, .. } =>
            ("IndexOutOfBounds".into(), format!("index {index}, length {length}")),
        RuntimeError::DivisionByZero { .. } =>
            ("DivisionByZero".into(), "division by zero".into()),
        RuntimeError::UnwrapNone { .. } =>
            ("UnwrapNone".into(), "unwrap on None".into()),
        RuntimeError::UnwrapErr { .. } =>
            ("UnwrapErr".into(), "unwrap on Err".into()),
        RuntimeError::TypeMismatch { expected, got, .. } =>
            ("TypeMismatch".into(), format!("expected {expected}, got {got}")),
        RuntimeError::UndefinedVariable { name, .. } =>
            ("UndefinedVariable".into(), format!("undefined variable '{name}'")),
        RuntimeError::ReentrantLock { .. } =>
            ("ReentrantLock".into(), "reentrant lock".into()),
        RuntimeError::UnlockNotOwned { .. } =>
            ("UnlockNotOwned".into(), "unlock not owned".into()),
        RuntimeError::ExposeMutableFieldReplace { .. } =>
            ("ExposeMutableFieldReplace".into(), "cannot replace expose_mutable field from outside".into()),
        RuntimeError::HandlerDispatch(de) => {
            let (kind, msg) = dispatch_error_to_kind(de);
            (kind, msg)
        }
        RuntimeError::Structured { kind, message, .. } =>
            (kind.clone(), message.clone()),
    };
    Value::ErrorObj { kind, message }
}

/// Collect Arc<dyn BreakablePrimitive> for all define fields in self_map that hold
/// a Signal, Contract, or Permit. Used for scope binding in exec_scope_block.
fn collect_define_field_prims(
    self_map: &Rc<RefCell<HashMap<String, Value>>>,
    define_names: &[String],
) -> Vec<Arc<dyn BreakablePrimitive>> {
    let map = self_map.borrow();
    let mut prims: Vec<Arc<dyn BreakablePrimitive>> = Vec::new();
    for name in define_names {
        if let Some(val) = map.get(name) {
            match val {
                Value::Signal(s)   => prims.push(Arc::clone(s) as Arc<dyn BreakablePrimitive>),
                Value::Contract(c) => prims.push(Arc::clone(c) as Arc<dyn BreakablePrimitive>),
                Value::Permit(p)   => prims.push(Arc::clone(p) as Arc<dyn BreakablePrimitive>),
                _ => {}
            }
        }
    }
    prims
}


// ── Call-stack frames (for runtime tracebacks) ───────────────────────────────

/// A single entry in a runtime traceback: the name of the function/handler and
/// the source span of its definition (or the call site).
#[derive(Debug, Clone)]
pub struct Frame {
    pub name: String,
    pub span: Span,
}

// ── Interpreter state (shared via Rc between main body task + handler tasks) ──

/// Shared mutable field storage for a template instance; all mini-threads from
/// the same template hold an `Rc` to this map.
type TemplateSelf = RefCell<Option<Rc<RefCell<HashMap<String, Value>>>>>;

pub struct InterpState {
    pub env: RefCell<Env>,
    pub func_table: RefCell<HashMap<String, Arc<FuncDef>>>,
    pub scope_templates: RefCell<HashMap<String, Arc<ScopeTemplateDecl>>>,
    pub thread_templates: RefCell<HashMap<String, Arc<ThreadTemplateDecl>>>,
    pub current_thread_state: RefCell<Option<Arc<ThreadState>>>,
    pub expose_field_names: RefCell<HashSet<String>>,
    pub expose_mutable_field_names: RefCell<HashSet<String>>,
    /// Shared field storage for the current thread template instance.
    /// All mini-threads spawned from this template share the same Rc.
    pub template_self: TemplateSelf,
    /// Live call stack for the current task, pushed/popped around function and
    /// handler invocations. Each task (thread/mini-thread) has its own.
    pub call_stack: RefCell<Vec<Frame>>,
    /// Snapshot of `call_stack` captured at the deepest point an error was
    /// first observed, used to render a traceback. Set once per failure.
    pub last_backtrace: RefCell<Option<Vec<Frame>>>,
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
            template_self: RefCell::new(None),
            call_stack: RefCell::new(Vec::new()),
            last_backtrace: RefCell::new(None),
        }
    }

    /// Create a child state for a spawned Tessera thread: shares template tables, fresh env.
    pub fn new_for_thread(parent: &Rc<InterpState>) -> Rc<InterpState> {
        Rc::new(InterpState {
            env: RefCell::new(Env::new()),
            func_table: RefCell::new(parent.func_table.borrow().clone()),
            scope_templates: RefCell::new(parent.scope_templates.borrow().clone()),
            thread_templates: RefCell::new(parent.thread_templates.borrow().clone()),
            current_thread_state: RefCell::new(None),
            expose_field_names: RefCell::new(HashSet::new()),
            expose_mutable_field_names: RefCell::new(HashSet::new()),
            template_self: RefCell::new(None),
            call_stack: RefCell::new(Vec::new()),
            last_backtrace: RefCell::new(None),
        })
    }

    /// Create a child state for a mini-thread (top-level async function call):
    /// fresh local env, but shares template_self Rc and thread-state for expose sync.
    pub fn new_for_mini_thread(parent: &Rc<InterpState>) -> Rc<InterpState> {
        Rc::new(InterpState {
            env: RefCell::new(Env::new()),
            func_table: RefCell::new(parent.func_table.borrow().clone()),
            scope_templates: RefCell::new(parent.scope_templates.borrow().clone()),
            thread_templates: RefCell::new(parent.thread_templates.borrow().clone()),
            // Share thread state so expose sync works from mini-threads
            current_thread_state: RefCell::new(parent.current_thread_state.borrow().clone()),
            expose_field_names: RefCell::new(parent.expose_field_names.borrow().clone()),
            expose_mutable_field_names: RefCell::new(parent.expose_mutable_field_names.borrow().clone()),
            // Share the same Rc so mini-threads see template fields
            template_self: RefCell::new(parent.template_self.borrow().clone()),
            call_stack: RefCell::new(Vec::new()),
            last_backtrace: RefCell::new(None),
        })
    }
}

// ── Interpreter ───────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct Interpreter(pub Rc<InterpState>);

impl Default for Interpreter {
    fn default() -> Self { Self::new() }
}

impl Interpreter {
    pub fn new() -> Self {
        Self(Rc::new(InterpState::new()))
    }

    pub fn new_for_thread(parent: &Interpreter) -> Self {
        Self(InterpState::new_for_thread(&parent.0))
    }

    pub fn new_for_mini_thread(parent: &Interpreter) -> Self {
        Self(InterpState::new_for_mini_thread(&parent.0))
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
                        // Type guard: new value must match the type of the existing variable.
                        // Allow void -> T (void is the sentinel for uninitialized define fields).
                        if let Some(existing) = self.0.env.borrow().lookup(&i.name).cloned() {
                            let old_ty = existing.type_name();
                            let new_ty = v.type_name();
                            if old_ty != "void" && old_ty != new_ty {
                                return Err(RuntimeError::Panic {
                                    message: format!(
                                        "cannot assign {} to variable '{}' of type {}",
                                        new_ty, i.name, old_ty
                                    ),
                                    location: a.span,
                                });
                            }
                        }
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
                        match obj {
                            Value::Object(map) => {
                                // self.fieldName = value inside a template method
                                map.borrow_mut().insert(field.name.clone(), v.clone());
                                self.maybe_sync_expose(&field.name, &v);
                            }
                            Value::ThreadHandle(state) => {
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
                            other => {
                                return Err(RuntimeError::Panic {
                                    message: format!("field assignment on non-object type {}", other.type_name()),
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
            if let Some(v) = self.exec_stmt(s).await? {
                self.0.env.borrow_mut().pop_scope();
                return Ok(Some(v));
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

        // Scope-bound primitives collected after __on_enter__ runs.
        let mut scope_prims: Vec<Arc<dyn BreakablePrimitive>> = Vec::new();

        if let Some(decl) = &decl {
            // Collect define field names for scope binding (before anything else).
            let define_field_names: Vec<String> = decl.members.iter()
                .filter_map(|m| if let ScopeTemplateMember::Define(e) = m { Some(e.name.name.clone()) } else { None })
                .collect();

            // Build self_map for scope template fields so `self.xxx` works.
            let self_map: std::rc::Rc<std::cell::RefCell<std::collections::HashMap<String, Value>>> =
                std::rc::Rc::new(std::cell::RefCell::new(std::collections::HashMap::new()));
            for (param, arg_expr) in decl.params.iter().zip(sb.args.iter()) {
                let v = self.eval_expr(arg_expr).await?;
                self_map.borrow_mut().insert(param.name.name.clone(), v);
            }
            // Initialize define fields before __on_enter__
            for m in &decl.members {
                if let ScopeTemplateMember::Define(e) = m {
                    let val = if let Some(init) = &e.initializer {
                        self.eval_expr(init).await?
                    } else {
                        crate::event_loop::default_value_for_type(&e.ty)
                    };
                    self_map.borrow_mut().insert(e.name.name.clone(), val);
                }
            }
            // Bind `self` in env so __on_enter__ / __on_exit__ / body can access fields via self.xxx.
            self.0.env.borrow_mut().define("self".to_string(), Value::Object(self_map.clone()));
            // Run __on_enter__
            if let Some(on_enter) = find_scope_hook(decl, "__on_enter__") {
                if let Err(e) = self.exec_block(&on_enter.body).await {
                    // __on_enter__ crashed — break whatever scope prims were created, then propagate.
                    let prims = collect_define_field_prims(&self_map, &define_field_names);
                    for p in &prims { p.break_with(BrokenReason::ScopeCrashed); }
                    self.0.env.borrow_mut().pop_scope();
                    return Err(e);
                }
            }

            // Collect scope-bound primitives AFTER __on_enter__ succeeds.
            scope_prims = collect_define_field_prims(&self_map, &define_field_names);
        }

        // Run user body (save error for after on_exit)
        let body_result = self.exec_block(&sb.body).await;
        let body_crashed = body_result.is_err();

        // Run __on_exit__ unconditionally as cleanup, but no longer swallow its
        // error: capture it so it can be surfaced if the body itself succeeded.
        let mut exit_result: Result<(), RuntimeError> = Ok(());
        if let Some(decl) = &decl {
            if let Some(on_exit) = find_scope_hook(decl, "__on_exit__") {
                if let Err(e) = self.exec_block(&on_exit.body).await {
                    exit_result = Err(e);
                }
            }
            // Break scope-bound primitives AFTER __on_exit__ returns.
            // ScopeGone = normal exit; ScopeCrashed = enclosing thread was crashing.
            let reason = if body_crashed { BrokenReason::ScopeCrashed } else { BrokenReason::ScopeGone };
            for p in &scope_prims {
                p.break_with(reason.clone());
            }
        }

        self.0.env.borrow_mut().pop_scope();

        // The body error happened first, so it takes precedence; otherwise
        // propagate any error raised by __on_exit__.
        match body_result {
            Err(e) => Err(e),
            Ok(v) => exit_result.map(|()| v),
        }
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

        let is_terminatable = decl.as_ref().is_some_and(|d| {
            d.members.iter().any(|m| matches!(m, tessera_ast::ThreadTemplateMember::OnTerminate(_)))
        });

        let (handler_tx, handler_rx) = mpsc::channel::<HandlerRequest>(64);
        let thread_state = ThreadState::new(template_name, handler_tx, is_terminatable);

        let child_state = InterpState::new_for_thread(&self.0);
        let child_interp = Interpreter(child_state);
        let body = Arc::new(ts.body.clone());
        let state_clone = thread_state.clone();

        // Channel that fires once __on_enter__ has finished (successfully or not).
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<()>();

        tokio::task::spawn_local(crate::event_loop::run_thread_task(
            child_interp,
            decl,
            arg_values,
            body,
            state_clone,
            handler_rx,
            ready_tx,
        ));

        // Wait for __on_enter__ to complete before binding the handle.
        // This matches the spec guarantee that __on_enter__ finishes before
        // the parent thread continues, regardless of how many await points
        // __on_enter__ contains.
        let _ = ready_rx.await;

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
                // `self` is a special identifier — always refers to the current template instance.
                if i.name == "self" {
                    if let Some(obj) = self.0.template_self.borrow().clone() {
                        return Ok(Value::Object(obj));
                    }
                    // Fall back to env lookup (scope template binds `self` as Value::Object in env).
                    if let Some(val) = self.0.env.borrow().lookup("self").cloned() {
                        return Ok(val);
                    }
                    return Err(RuntimeError::Panic {
                        message: "`self` is not available outside a template".into(),
                        location: i.span,
                    });
                }
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
            Expr::Try(t) => self.eval_try(t).await,
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
                    use std::io::Write;
                    let _ = std::io::stdout().flush();
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
                    // Release the lock before yielding so the mutex is not held
                    // across an unrelated await point.
                    let ch = {
                        let mut rx = stdin_receiver().lock().await;
                        rx.recv().await.flatten()
                    };
                    // Yield after every char so other tasks (e.g. handlers) get
                    // a chance to run even when the stdin channel has data buffered.
                    tokio::task::yield_now().await;
                    return Ok(match ch {
                        Some(c) => Value::Option(Some(Box::new(Value::Char(c)))),
                        None => Value::Option(None),
                    });
                }
                "signal" => {
                    return Ok(Value::Signal(Arc::new(TesseraSignal::new())));
                }
                "contract" => {
                    return Ok(Value::Contract(Arc::new(TesseraContract::new())));
                }
                "permit" => {
                    let initial = if let Some(arg) = c.args.first() {
                        match self.eval_expr(arg).await? {
                            Value::Int(n) => n,
                            _ => return Err(RuntimeError::Panic {
                                message: "permit: initial must be an int".into(),
                                location: c.span,
                            }),
                        }
                    } else {
                        0
                    };
                    if initial < 0 {
                        return Err(RuntimeError::Panic {
                            message: format!("permit: initial must be non-negative, got {initial}"),
                            location: c.span,
                        });
                    }
                    return Ok(Value::Permit(Arc::new(TesseraPermit::new(initial))));
                }
                name => {
                    // Look up user-defined function
                    let func = self.0.func_table.borrow().get(name).cloned();
                    if let Some(func) = func {
                        let mut args = Vec::new();
                        for a in &c.args { args.push(self.eval_expr(a).await?); }
                        if func.kind == FuncKind::Async {
                            // All async function calls spawn a mini-thread.
                            // The mini-thread shares template_self (if any) and current_thread_state
                            // for expose sync, but gets a fresh local variable env.
                            let (tx, rx) = tokio::sync::oneshot::channel::<FutureOutcome>();
                            let child = Interpreter::new_for_mini_thread(self);
                            // Bind `self` in the mini-thread's env if we have a template_self.
                            if let Some(obj) = child.0.template_self.borrow().clone() {
                                child.0.env.borrow_mut().define("self".to_string(), Value::Object(obj));
                            }
                            tokio::task::spawn_local(async move {
                                let outcome = match child.call_func(&func, args).await {
                                    Ok(v)  => FutureOutcome::Ok(v),
                                    Err(e) => FutureOutcome::Failed(e.to_string()),
                                };
                                let _ = tx.send(outcome);
                            });
                            return Ok(Value::Future(TesseraFuture::new(rx)));
                        }
                        let result = self.call_func(&func, args).await;
                        return result;
                    }
                    return Err(RuntimeError::Panic {
                        message: format!("unknown function '{name}'"),
                        location: c.span,
                    });
                }
            }
        }
        Ok(Value::Void)
    }

    // ── Call-stack bookkeeping for tracebacks ───────────────────────────────

    fn push_frame(&self, name: impl Into<String>, span: Span) {
        self.0.call_stack.borrow_mut().push(Frame { name: name.into(), span });
    }

    fn pop_frame(&self) {
        self.0.call_stack.borrow_mut().pop();
    }

    /// Snapshot the current call stack into `last_backtrace` the first time an
    /// error is seen, so the traceback reflects the deepest point of failure.
    fn record_backtrace(&self) {
        let mut bt = self.0.last_backtrace.borrow_mut();
        if bt.is_none() {
            *bt = Some(self.0.call_stack.borrow().clone());
        }
    }

    pub async fn call_func(&self, func: &FuncDef, args: Vec<Value>) -> Result<Value, RuntimeError> {
        self.push_frame(func.name.name.clone(), func.span);
        let result = self.call_func_inner(func, args).await;
        if result.is_err() {
            self.record_backtrace();
        }
        self.pop_frame();
        result
    }

    async fn call_func_inner(&self, func: &FuncDef, args: Vec<Value>) -> Result<Value, RuntimeError> {
        // Arity check
        let min_args = func.params.iter().filter(|p| p.default.is_none()).count();
        let max_args = func.params.len();
        if args.len() < min_args || args.len() > max_args {
            return Err(RuntimeError::Panic {
                message: format!(
                    "function '{}' expects {} argument(s), got {}",
                    func.name.name, max_args, args.len()
                ),
                location: func.span,
            });
        }
        // Type check each argument against its declared parameter type
        for (param, val) in func.params.iter().zip(args.iter()) {
            if !runtime_type_matches(&param.ty, val) {
                return Err(RuntimeError::Panic {
                    message: format!(
                        "argument '{}': expected {}, got {}",
                        param.name.name,
                        type_expr_display(&param.ty),
                        val.type_name()
                    ),
                    location: param.span,
                });
            }
        }
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
            // ── Option ────────────────────────────────────────────────────────
            ("isSome", Value::Option(v)) => Ok(Value::Bool(v.is_some())),
            ("isNone", Value::Option(v)) => Ok(Value::Bool(v.is_none())),
            ("unwrap", Value::Option(Some(v))) => Ok(*v),
            ("unwrap", Value::Option(None)) => Err(RuntimeError::UnwrapNone { location: m.span }),
            ("unwrapOr", Value::Option(Some(v))) => Ok(*v),
            ("unwrapOr", Value::Option(None)) => Ok(args.into_iter().next().unwrap_or(Value::Void)),

            // ── Result ────────────────────────────────────────────────────────
            ("isOk",  Value::Result(r)) => Ok(Value::Bool(r.is_ok())),
            ("isErr", Value::Result(r)) => Ok(Value::Bool(r.is_err())),
            ("unwrap",    Value::Result(Ok(v)))  => Ok(*v),
            ("unwrap",    Value::Result(Err(_))) => Err(RuntimeError::UnwrapErr { location: m.span }),
            ("unwrapErr", Value::Result(Err(e))) => Ok(*e),
            ("unwrapErr", Value::Result(Ok(_)))  => Err(RuntimeError::UnwrapErr { location: m.span }),
            ("unwrapOr",  Value::Result(Ok(v)))  => Ok(*v),
            ("unwrapOr",  Value::Result(Err(_))) => Ok(args.into_iter().next().unwrap_or(Value::Void)),

            // ── List ──────────────────────────────────────────────────────────
            ("length", Value::List(l)) => Ok(Value::Int(l.borrow().len() as i32)),
            ("length", Value::Str(s))  => Ok(Value::Int(s.chars().count() as i32)),
            ("isEmpty", Value::List(l)) => Ok(Value::Bool(l.borrow().is_empty())),

            // ── Type conversions ───────────────────────────────────────────────
            ("toString", Value::Int(n))    => Ok(Value::Str(n.to_string())),
            ("toString", Value::Double(f)) => Ok(Value::Str(f.to_string())),
            ("toString", Value::Char(c))   => Ok(Value::Str(c.to_string())),
            ("toString", Value::Bool(b))   => Ok(Value::Str(b.to_string())),
            ("toInt",    Value::Double(f)) => Ok(Value::Int(f as i32)),
            ("toInt",    Value::Char(c))   => Ok(Value::Int(c as i32)),
            ("toInt",    Value::Str(s)) => Ok(Value::Result(
                s.trim().parse::<i32>()
                    .map(|n| Box::new(Value::Int(n)))
                    .map_err(|e| Box::new(Value::Str(e.to_string())))
            )),
            ("toDouble", Value::Int(n))  => Ok(Value::Double(n as f64)),
            ("toDouble", Value::Str(s))  => Ok(Value::Result(
                s.trim().parse::<f64>()
                    .map(|f| Box::new(Value::Double(f)))
                    .map_err(|e| Box::new(Value::Str(e.to_string())))
            )),
            ("toChar",   Value::Int(n)) => Ok(Value::Option(
                char::from_u32(n as u32).map(|c| Box::new(Value::Char(c)))
            )),
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
                        return Err(RuntimeError::IndexOutOfBounds {
                            index: *i, length: l.len() as i32, location: m.span,
                        });
                    }
                    Ok(l[idx].clone())
                } else {
                    Err(RuntimeError::Panic {
                        message: "List.get() requires an int index".into(),
                        location: m.span,
                    })
                }
            }
            ("set", Value::List(l)) => {
                if let (Some(Value::Int(i)), Some(v)) = (args.first(), args.get(1)) {
                    let mut l = l.borrow_mut();
                    let idx = *i as usize;
                    if idx >= l.len() {
                        return Err(RuntimeError::IndexOutOfBounds {
                            index: *i, length: l.len() as i32, location: m.span,
                        });
                    }
                    l[idx] = v.clone();
                    Ok(Value::Void)
                } else {
                    Err(RuntimeError::Panic {
                        message: "List.set() requires an int index and a value".into(),
                        location: m.span,
                    })
                }
            }

            // ── Map ───────────────────────────────────────────────────────────
            ("size", Value::Map(m_val)) => Ok(Value::Int(m_val.borrow().len() as i32)),
            ("get", Value::Map(m_val)) => {
                let key = args.into_iter().next();
                let result = key.and_then(|k| {
                    ValueKey::try_from(k).ok()
                        .and_then(|vk| m_val.borrow().get(&vk).cloned())
                });
                Ok(Value::Option(result.map(Box::new)))
            }
            ("set", Value::Map(m_val)) => {
                if let (Some(key), Some(val)) = (args.first().cloned(), args.get(1).cloned()) {
                    match ValueKey::try_from(key) {
                        Ok(vk) => { m_val.borrow_mut().insert(vk, val); }
                        Err(_) => return Err(RuntimeError::Panic {
                            message: "Map.set() key type is not hashable (must be bool/int/char/String)".into(),
                            location: m.span,
                        }),
                    }
                } else {
                    return Err(RuntimeError::Panic {
                        message: "Map.set() requires a key and a value".into(),
                        location: m.span,
                    });
                }
                Ok(Value::Void)
            }
            ("remove", Value::Map(m_val)) => {
                let key = args.into_iter().next();
                let removed = key.and_then(|k| {
                    ValueKey::try_from(k).ok()
                        .and_then(|vk| m_val.borrow_mut().shift_remove(&vk))
                });
                Ok(Value::Option(removed.map(Box::new)))
            }

            // ── Future .wait() / .isDone() ───────────────────────────────────
            ("wait", Value::Future(fut)) => {
                match fut.resolve().await {
                    FutureOutcome::Ok(v) => Ok(v),
                    FutureOutcome::Failed(msg) => Err(RuntimeError::Panic { message: msg, location: m.span }),
                }
            }
            ("isDone", Value::Future(fut)) => {
                Ok(Value::Bool(fut.is_done()))
            }

            // ── HandlerFuture .wait() (sync) / state checks ───────────────────
            ("wait", Value::HandlerFuture(hf)) => {
                match hf.resolve().await {
                    HandlerResolveResult::Ok(v) => Ok(v),
                    HandlerResolveResult::DispatchFailed(e) => {
                        let (kind, message) = dispatch_error_to_kind(&e);
                        Err(RuntimeError::Structured { kind, message, location: m.span })
                    }
                    HandlerResolveResult::ExecutionFailed(msg) =>
                        Err(RuntimeError::Structured {
                            kind: "ExecutionFailed".into(),
                            message: msg,
                            location: m.span,
                        }),
                }
            }
            ("isDone", Value::HandlerFuture(hf)) => Ok(Value::Bool(hf.is_done())),
            ("isOk",   Value::HandlerFuture(hf)) => Ok(Value::Bool(hf.is_ok())),
            ("isErr",  Value::HandlerFuture(hf)) => Ok(Value::Bool(hf.is_err())),

            // ── ThreadHandle .terminate() ─────────────────────────────────────
            ("terminate", Value::ThreadHandle(state)) => {
                if !state.is_terminatable {
                    return Err(RuntimeError::Panic {
                        message: format!(
                            "thread '{}' is not terminatable (no __on_terminate__ declared)",
                            state.template_name.as_deref().unwrap_or("<anonymous>")
                        ),
                        location: m.span,
                    });
                }
                let fut = state.terminate().await;
                Ok(Value::Future(fut))
            }

            // ── ThreadHandle handler calls ────────────────────────────────────
            (method, Value::ThreadHandle(state)) => {
                let hf_result = state.dispatch_handler(method.to_string(), args).await;
                match hf_result {
                    Ok(fut) => Ok(Value::HandlerFuture(tessera_runtime::TesseraHandlerFuture::from_future(fut))),
                    Err(e)  => Ok(Value::HandlerFuture(tessera_runtime::TesseraHandlerFuture::rejected(e))),
                }
            }

            // ── Queue ─────────────────────────────────────────────────────────
            ("push", Value::Queue(q)) => {
                let v = args.into_iter().next().unwrap_or(Value::Void);
                match q.push(v) {
                    Ok(()) => Ok(Value::Result(Ok(Box::new(Value::Void)))),
                    Err(QueuePushError::Full)   => Ok(Value::Result(Err(Box::new(Value::Str("Full".into()))))),
                    Err(QueuePushError::Closed) => Ok(Value::Result(Err(Box::new(Value::Str("Closed".into()))))),
                }
            }
            ("enqueue", Value::Queue(q)) => {
                let v = args.into_iter().next().unwrap_or(Value::Void);
                if !q.enqueue(v).await {
                    return Err(RuntimeError::Panic {
                        message: "QueueClosed".into(),
                        location: m.span,
                    });
                }
                Ok(Value::Void)
            }
            ("dequeue", Value::Queue(q)) => Ok(Value::Option(q.dequeue().await.map(Box::new))),
            ("tryPush", Value::Queue(q)) => {
                let v = args.into_iter().next().unwrap_or(Value::Void);
                Ok(Value::Bool(q.try_push(v)))
            }
            ("tryPop", Value::Queue(q)) => Ok(Value::Option(q.try_pop().map(Box::new))),
            ("size",    Value::Queue(q)) => Ok(Value::Int(q.size() as i32)),
            ("isEmpty", Value::Queue(q)) => Ok(Value::Bool(q.is_empty())),
            ("isClosed",Value::Queue(q)) => Ok(Value::Bool(q.is_closed())),
            ("waitForNonEmpty", Value::Queue(q)) => { q.wait_for_non_empty().await; Ok(Value::Void) }
            ("close",   Value::Queue(q)) => { q.close(); Ok(Value::Void) }

            // ── locked<T> ─────────────────────────────────────────────────────
            ("lock", Value::Locked(l)) => {
                let owner_id = self.current_thread_id();
                match l.lock(owner_id).await {
                    Ok(()) => Ok(Value::Void),
                    Err(()) => Err(RuntimeError::ReentrantLock { location: m.span }),
                }
            }
            ("tryLock", Value::Locked(l)) => {
                let owner_id = self.current_thread_id();
                match l.try_lock(owner_id) {
                    Ok(acquired) => Ok(Value::Bool(acquired)),
                    Err(()) => Err(RuntimeError::ReentrantLock { location: m.span }),
                }
            }
            ("unlock", Value::Locked(l)) => {
                let owner_id = self.current_thread_id();
                if l.unlock(owner_id) {
                    Ok(Value::Void)
                } else {
                    Err(RuntimeError::UnlockNotOwned { location: m.span })
                }
            }
            ("isLocked", Value::Locked(l)) => Ok(Value::Bool(l.is_locked())),
            ("get", Value::Locked(l)) => Ok(l.get().await),
            ("set", Value::Locked(l)) => {
                let v = args.into_iter().next().unwrap_or(Value::Void);
                l.set(v).await;
                Ok(Value::Void)
            }

            // ── signal ────────────────────────────────────────────────────────
            ("raise",    Value::Signal(s)) => { s.raise();  Ok(Value::Void) }
            ("reset",    Value::Signal(s)) => { s.reset();  Ok(Value::Void) }
            ("isRaised", Value::Signal(s)) => Ok(Value::Bool(s.is_raised())),
            ("isOk",     Value::Signal(s)) => Ok(Value::Bool(s.is_ok())),
            ("isErr",    Value::Signal(s)) => Ok(Value::Bool(s.is_err())),
            ("wait",     Value::Signal(s)) => {
                match s.wait().await {
                    Ok(()) => Ok(Value::Void),
                    Err(r) => Err(RuntimeError::Structured {
                        kind: r.as_str().into(),
                        message: format!("signal broken: {}", r.as_str()),
                        location: m.span,
                    }),
                }
            }

            // ── contract ──────────────────────────────────────────────────────
            ("fulfill",   Value::Contract(c)) => { c.fulfill(); Ok(Value::Void) }
            ("isPending", Value::Contract(c)) => Ok(Value::Bool(c.is_pending())),
            ("isOk",      Value::Contract(c)) => Ok(Value::Bool(c.is_ok())),
            ("isErr",     Value::Contract(c)) => Ok(Value::Bool(c.is_err())),
            ("wait",      Value::Contract(c)) => {
                match c.wait().await {
                    Ok(()) => Ok(Value::Void),
                    Err(r) => Err(RuntimeError::Structured {
                        kind: r.as_str().into(),
                        message: format!("contract broken: {}", r.as_str()),
                        location: m.span,
                    }),
                }
            }

            // ── permit ────────────────────────────────────────────────────────
            ("release", Value::Permit(p)) => {
                let n = args.into_iter().next();
                match n {
                    Some(Value::Int(n)) => {
                        if n <= 0 {
                            return Err(RuntimeError::Panic {
                                message: format!("permit.release(n): n must be positive, got {n}"),
                                location: m.span,
                            });
                        }
                        p.release_n(n);
                    }
                    None => p.release(),
                    _ => return Err(RuntimeError::Panic {
                        message: "permit.release(n): n must be an int".into(),
                        location: m.span,
                    }),
                }
                Ok(Value::Void)
            }
            ("count",   Value::Permit(p)) => Ok(Value::Int(p.count())),
            ("isOk",   Value::Permit(p)) => Ok(Value::Bool(p.is_ok())),
            ("isErr",  Value::Permit(p)) => Ok(Value::Bool(p.is_err())),
            ("wait",   Value::Permit(p)) => {
                match p.acquire().await {
                    Ok(()) => Ok(Value::Void),
                    Err(r) => Err(RuntimeError::Structured {
                        kind: r.as_str().into(),
                        message: format!("permit broken: {}", r.as_str()),
                        location: m.span,
                    }),
                }
            }

            (method, recv) => Err(RuntimeError::Panic {
                message: format!("no method '{}' on type {}", method, recv.type_name()),
                location: m.span,
            }),
        }
    }

    /// Returns the current Tessera thread's stable identifier (Arc pointer address).
    /// Used as owner_id for `locked<T>` explicit locking.
    fn current_thread_id(&self) -> usize {
        self.0.current_thread_state.borrow()
            .as_ref()
            .map(|arc| Arc::as_ptr(arc) as usize)
            .unwrap_or(0)
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
                match val {
                    Some(v) => Ok(v),
                    None => Err(RuntimeError::Panic {
                        message: format!("no field '{}' on thread handle", f.field.name),
                        location: f.span,
                    }),
                }
            }
            Value::Object(map) => {
                match map.borrow().get(&f.field.name).cloned() {
                    Some(v) => Ok(v),
                    None => Err(RuntimeError::Panic {
                        message: format!("no field '{}' on self", f.field.name),
                        location: f.span,
                    }),
                }
            }
            Value::ErrorObj { kind, message } => match f.field.name.as_str() {
                "kind"    => Ok(Value::Str(kind)),
                "message" => Ok(Value::Str(message)),
                other => Err(RuntimeError::Panic {
                    message: format!("no field '{}' on error", other),
                    location: f.span,
                }),
            },
            other => Err(RuntimeError::Panic {
                message: format!("field access on non-thread type {}", other.type_name()),
                location: f.span,
            }),
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
                    return Err(RuntimeError::IndexOutOfBounds { index: n, length: l.len() as i32, location: i.span });
                }
                Ok(l[ui].clone())
            }
            (Value::Str(s), Value::Int(n)) => {
                let ui = n as usize;
                let chars: Vec<char> = s.chars().collect();
                if ui >= chars.len() {
                    return Err(RuntimeError::IndexOutOfBounds { index: n, length: chars.len() as i32, location: i.span });
                }
                Ok(Value::Char(chars[ui]))
            }
            (obj, idx) => Err(RuntimeError::Panic {
                message: format!("index operator not supported: {}[{}]", obj.type_name(), idx.type_name()),
                location: i.span,
            }),
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
                HandlerResolveResult::Ok(v) => Ok(v),
                HandlerResolveResult::DispatchFailed(e) => {
                    let (kind, message) = dispatch_error_to_kind(&e);
                    Err(RuntimeError::Structured { kind, message, location: a.span })
                }
                HandlerResolveResult::ExecutionFailed(msg) =>
                    Err(RuntimeError::Structured {
                        kind: "ExecutionFailed".into(),
                        message: msg,
                        location: a.span,
                    }),
            },
            // `await s` — panics if signal is broken
            Value::Signal(s) => {
                match s.wait().await {
                    Ok(()) => Ok(Value::Void),
                    Err(r) => Err(RuntimeError::Structured {
                        kind: r.as_str().into(),
                        message: format!("signal broken: {}", r.as_str()),
                        location: a.span,
                    }),
                }
            }
            // `await c` — panics if contract is broken
            Value::Contract(c) => {
                match c.wait().await {
                    Ok(()) => Ok(Value::Void),
                    Err(r) => Err(RuntimeError::Structured {
                        kind: r.as_str().into(),
                        message: format!("contract broken: {}", r.as_str()),
                        location: a.span,
                    }),
                }
            }
            // `await p` — panics if permit is broken
            Value::Permit(p) => {
                match p.acquire().await {
                    Ok(()) => Ok(Value::Void),
                    Err(r) => Err(RuntimeError::Structured {
                        kind: r.as_str().into(),
                        message: format!("permit broken: {}", r.as_str()),
                        location: a.span,
                    }),
                }
            }
            other => Ok(other),
        }
    }

    async fn eval_try(&self, t: &TryExpr) -> Result<Value, RuntimeError> {
        match self.eval_expr(&t.expr).await {
            Ok(v) => Ok(Value::Result(Ok(Box::new(v)))),
            Err(e) => {
                let err_obj = runtime_error_to_error_obj(&e);
                Ok(Value::Result(Err(Box::new(err_obj))))
            }
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
            "Map"  => Ok(Value::Map(Rc::new(RefCell::new(indexmap::IndexMap::new())))),
            "locked" => {
                let initial = args.into_iter().next().unwrap_or(Value::Void);
                Ok(Value::Locked(Arc::new(TesseraLocked::new(initial))))
            }
            "Queue" => {
                let capacity = args.first()
                    .and_then(|v| if let Value::Int(n) = v { Some(*n) } else { None })
                    .unwrap_or(0);
                Ok(Value::Queue(Arc::new(TesseraQueue::new(capacity))))
            }
            name => Err(RuntimeError::Panic {
                message: format!("unknown type constructor '{name}'"),
                location: tc.span,
            }),
        }
    }

    // ── Expose field sync helper ───────────────────────────────────────────────

    // Non-async: uses try_write() so the thread body never yields here.
    // try_write() always succeeds (no contention — only this task writes).
    fn maybe_sync_expose(&self, name: &str, value: &Value) {
        let is_expose         = self.0.expose_field_names.borrow().contains(name);
        let is_expose_mutable = !is_expose && self.0.expose_mutable_field_names.borrow().contains(name);

        if !is_expose && !is_expose_mutable { return; }

        if let Some(state) = self.0.current_thread_state.borrow().clone() {
            // Sync the field value into the expose map.
            let fields = if is_expose { &state.expose_fields } else { &state.expose_mutable_fields };
            if let Ok(mut guard) = fields.try_write() {
                guard.insert(name.to_string(), value.clone());
            }

            // Claim binding ownership for primitive values (first expose wins).
            let owned: Option<Arc<dyn BreakablePrimitive>> = match value {
                Value::Signal(s)   if s.try_claim_ownership() => Some(Arc::clone(s) as _),
                Value::Contract(c) if c.try_claim_ownership() => Some(Arc::clone(c) as _),
                Value::Permit(p)   if p.try_claim_ownership() => Some(Arc::clone(p) as _),
                _ => None,
            };
            if let Some(prim) = owned {
                state.register_owned(prim);
            }
        }
    }

    // ── Public exec helpers (used by event loop) ─────────────────────────────

    pub async fn exec_func_def_body(&self, func: &FuncDef, args: Vec<Value>) -> Result<Value, RuntimeError> {
        self.call_func(func, args).await
    }

    pub async fn exec_handler_body(&self, handler: &HandlerDef, args: Vec<Value>) -> Result<Value, RuntimeError> {
        self.push_frame(format!("handler {}", handler.name.name), handler.span);
        self.0.env.borrow_mut().push_scope();
        for (param, val) in handler.params.iter().zip(args.into_iter()) {
            self.0.env.borrow_mut().define(param.name.name.clone(), val);
        }
        let result = self.exec_block(&handler.body).await;
        self.0.env.borrow_mut().pop_scope();
        if result.is_err() {
            self.record_backtrace();
        }
        self.pop_frame();
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

// ── Runtime argument type helpers ─────────────────────────────────────────────

/// Check if `val` is compatible with the declared parameter type `te`.
/// Unknown / unrecognised type annotations are skipped (returns true).
fn runtime_type_matches(te: &tessera_ast::TypeExpr, val: &Value) -> bool {
    match te {
        tessera_ast::TypeExpr::Void  => matches!(val, Value::Void),
        tessera_ast::TypeExpr::Never => false,
        tessera_ast::TypeExpr::Named(ident, _) => {
            let expected = match ident.name.as_str() {
                "bool"    => "bool",
                "int"     => "int",
                "double"  => "double",
                "char"    => "char",
                "String"  => "String",
                "void"    => return matches!(val, Value::Void),
                "never"   => return false,
                "List"    => "List",
                "Map"     => "Map",
                "Option"  => "Option",
                "Result"  => "Result",
                "locked"  => "locked",
                "Queue"   => "Queue",
                "signal"  => "signal",
                "contract" => "contract",
                "permit"  => "permit",
                "Future"  => "Future",
                "HandlerFuture" => "HandlerFuture",
                "thread"  => "ThreadHandle",
                _ => return true, // unknown annotation — don't reject
            };
            val.type_name() == expected
        }
    }
}

/// Human-readable name for a `TypeExpr`, used in error messages.
fn type_expr_display(te: &tessera_ast::TypeExpr) -> String {
    match te {
        tessera_ast::TypeExpr::Void => "void".to_string(),
        tessera_ast::TypeExpr::Never => "never".to_string(),
        tessera_ast::TypeExpr::Named(ident, args) if args.is_empty() => ident.name.clone(),
        tessera_ast::TypeExpr::Named(ident, args) => {
            let inner: Vec<String> = args.iter().map(type_expr_display).collect();
            format!("{}<{}>", ident.name, inner.join(", "))
        }
    }
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
        // HandlerFuture == Err(v): check if hf is in error state and error message matches
        (Value::HandlerFuture(hf), Value::Result(Err(e))) |
        (Value::Result(Err(e)), Value::HandlerFuture(hf)) => {
            match hf.get_err() {
                Some(msg) => values_equal(&Value::Str(msg), e),
                None => false,
            }
        }
        // signal/contract/permit == Err("OwnerGone" | "OwnerCrashed")
        (Value::Signal(s), Value::Result(Err(e))) |
        (Value::Result(Err(e)), Value::Signal(s)) => match s.broken_reason() {
            Some(r) => values_equal(&Value::Str(r.as_str().into()), e),
            None    => false,
        },
        (Value::Contract(c), Value::Result(Err(e))) |
        (Value::Result(Err(e)), Value::Contract(c)) => match c.broken_reason() {
            Some(r) => values_equal(&Value::Str(r.as_str().into()), e),
            None    => false,
        },
        (Value::Permit(p), Value::Result(Err(e))) |
        (Value::Result(Err(e)), Value::Permit(p)) => match p.broken_reason() {
            Some(r) => values_equal(&Value::Str(r.as_str().into()), e),
            None    => false,
        },
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
        Value::ErrorObj { kind, message } => format!("error({kind}: {message})"),
        _ => "<complex>".into(),
    }
}
