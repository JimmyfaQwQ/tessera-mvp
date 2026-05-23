/// Thread event loop — cooperative scheduling for Tessera threads.
///
/// Each Tessera thread maps to one `tokio::task::spawn_local` task running
/// inside a `LocalSet`. The loop:
///   1. Drives the main body future to completion
///   2. Dispatches handler requests inline (FIFO, cooperative)
///   3. Handles terminate() signals (drains queue, runs hooks, exits)
use std::sync::Arc;

use tessera_ast::{ThreadTemplateDecl, Block, ThreadTemplateMember, ThreadTemplateMember::*, TypeExpr, FuncDef};
use tessera_runtime::{
    ThreadState, ThreadStatus, HandlerRequest, HandlerOutcome,
    FutureOutcome, TesseraFuture, HandlerDispatchError, TerminateBundle, Value,
};
use tokio::sync::{mpsc, oneshot};

use crate::eval::{Interpreter, find_thread_hook, find_handler};

pub async fn run_thread_task(
    interp: Interpreter,
    decl: Option<Arc<ThreadTemplateDecl>>,
    args: Vec<Value>,
    body: Arc<Block>,
    state: Arc<ThreadState>,
    mut handler_rx: mpsc::Receiver<HandlerRequest>,
) {
    // ── Setup ────────────────────────────────────────────────────────────────

    *interp.0.current_thread_state.borrow_mut() = Some(state.clone());

    if let Some(d) = &decl {
        // Register member functions into func_table so the thread body can call them.
        for m in &d.members {
            let func: Option<&FuncDef> = match m {
                MemberFunc(f) | OnEnter(f) | OnExit(f) | OnTerminate(f) => Some(f),
                _ => None,
            };
            if let Some(f) = func {
                interp.0.func_table.borrow_mut().insert(f.name.name.clone(), Arc::new(f.clone()));
            }
        }

        // Initialize template_self: the shared Object that methods access via `self`.
        let self_map: std::rc::Rc<std::cell::RefCell<std::collections::HashMap<String, Value>>> =
            std::rc::Rc::new(std::cell::RefCell::new(std::collections::HashMap::new()));
        *interp.0.template_self.borrow_mut() = Some(self_map.clone());

        // Pre-populate template_self with default values for declared fields.
        for m in &d.members {
            match m {
                ThreadTemplateMember::Expose(e) => {
                    interp.0.expose_field_names.borrow_mut().insert(e.name.name.clone());
                    self_map.borrow_mut().insert(e.name.name.clone(), default_value_for_type(&e.ty));
                }
                ThreadTemplateMember::ExposeMutable(e) => {
                    interp.0.expose_mutable_field_names.borrow_mut().insert(e.name.name.clone());
                    self_map.borrow_mut().insert(e.name.name.clone(), default_value_for_type(&e.ty));
                }
                ThreadTemplateMember::Define(e) => {
                    let val = if let Some(init) = &e.initializer {
                        match interp.eval_expr(init).await {
                            Ok(v) => v,
                            Err(_) => default_value_for_type(&e.ty),
                        }
                    } else {
                        default_value_for_type(&e.ty)
                    };
                    self_map.borrow_mut().insert(e.name.name.clone(), val);
                }
                _ => {}
            }
        }
        // Bind template params into template_self (so mini-threads can access via self.paramName).
        for (param, val) in d.params.iter().zip(args.into_iter()) {
            self_map.borrow_mut().insert(param.name.name.clone(), val);
        }

        // Bind `self` in the interpreter env so template methods can access it.
        interp.0.env.borrow_mut().define("self".to_string(), Value::Object(self_map));
    }

    // ── __on_enter__ ─────────────────────────────────────────────────────────

    if let Some(d) = &decl {
        if let Some(hook) = find_thread_hook(d, "__on_enter__") {
            let hook = hook.clone();
            if let Err(e) = interp.exec_func_def_body(&hook, vec![]).await {
                state.set_status(ThreadStatus::Crashed(e.to_string())).await;
                drain_handlers(&mut handler_rx, HandlerDispatchError::TargetCrashed);
                if let Some(b) = state.take_terminate_bundle().await {
                    let _ = b.result_tx.send(FutureOutcome::Failed(e.to_string()));
                }
                return;
            }
        }
    }

    // ── Claim terminate channels (always present from ThreadState::new) ──────

    let TerminateBundle { signal_rx: mut terminate_rx, result_tx } = state
        .take_terminate_bundle()
        .await
        .expect("terminate bundle always present");
    let mut result_tx = Some(result_tx);

    // ── Main event loop ──────────────────────────────────────────────────────

    // Clone interp so the body future and handler dispatch use independent
    // Rust borrows while sharing the same Rc<InterpState>.
    let body_interp = interp.clone();
    let mut body_fut = body_interp.exec_block(&*body);

    loop {
        let exclusive = state.exclusive_mode();

        tokio::select! {
            biased;

            // ── Main body ────────────────────────────────────────────────────
            result = body_fut.as_mut() => {
                match result {
                    Ok(_) => {
                        run_hook(&interp, decl.as_deref(), "__on_exit__").await;
                        state.set_status(ThreadStatus::Terminated).await;
                        drain_handlers(&mut handler_rx, HandlerDispatchError::TargetTerminated);
                        if let Some(tx) = result_tx.take() {
                            let _ = tx.send(FutureOutcome::Ok(Value::Void));
                        }
                    }
                    Err(e) => {
                        state.set_status(ThreadStatus::Crashed(e.to_string())).await;
                        drain_handlers(&mut handler_rx, HandlerDispatchError::TargetCrashed);
                        if let Some(tx) = result_tx.take() {
                            let _ = tx.send(FutureOutcome::Failed(e.to_string()));
                        }
                    }
                }
                break;
            }

            // ── Handler dispatch (inline, cooperative) ───────────────────────
            req = handler_rx.recv(), if !exclusive => {
                if let Some(req) = req {
                    dispatch_handler_inline(&interp, decl.as_deref(), req);
                }
            }

            // ── Terminate signal ─────────────────────────────────────────────
            _ = &mut terminate_rx, if !exclusive => {
                state.set_status(ThreadStatus::Terminating).await;
                drain_handlers(&mut handler_rx, HandlerDispatchError::TargetTerminating);
                run_hook(&interp, decl.as_deref(), "__on_terminate__").await;
                run_hook(&interp, decl.as_deref(), "__on_exit__").await;
                state.set_status(ThreadStatus::Terminated).await;
                if let Some(tx) = result_tx.take() {
                    let _ = tx.send(FutureOutcome::Ok(Value::Void));
                }
                // body_fut is dropped here, abandoning the main body
                break;
            }
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Dispatch a handler request by spawning its body as an independent local task.
///
/// Running the handler inline (awaiting it directly in the event loop) would
/// block `body_fut` from being polled while the handler is executing. For
/// handlers that wait on state set by the body (e.g. `readLine` busy-waits on
/// `line_available` which is set by `run()`), this causes a deadlock. Spawning
/// the handler as a `spawn_local` task lets both the body and the handler be
/// scheduled cooperatively within the same `LocalSet`, so each can make progress
/// when the other yields.
fn dispatch_handler_inline(
    interp: &Interpreter,
    decl: Option<&ThreadTemplateDecl>,
    req: HandlerRequest,
) {
    let handler = decl.and_then(|d| find_handler(d, &req.handler_name));

    match handler {
        Some(h) => {
            let (exec_tx, exec_rx) = oneshot::channel::<FutureOutcome>();
            let _ = req.result_tx.send(HandlerOutcome::Dispatched(TesseraFuture::new(exec_rx)));

            let h = h.clone();
            let interp = interp.clone();
            let args = req.args;
            tokio::task::spawn_local(async move {
                match interp.exec_handler_body(&h, args).await {
                    Ok(v)  => { let _ = exec_tx.send(FutureOutcome::Ok(v)); }
                    Err(e) => { let _ = exec_tx.send(FutureOutcome::Failed(e.to_string())); }
                }
            });
        }
        None => {
            let _ = req.result_tx.send(HandlerOutcome::DispatchFailed(
                HandlerDispatchError::TargetCrashed,
            ));
        }
    }
}

/// Run a named lifecycle hook if present. Errors are silently ignored.
async fn run_hook(
    interp: &Interpreter,
    decl: Option<&ThreadTemplateDecl>,
    name: &str,
) {
    if let Some(d) = decl {
        if let Some(hook) = find_thread_hook(d, name) {
            let hook = hook.clone();
            let _ = interp.exec_func_def_body(&hook, vec![]).await;
        }
    }
}

/// Reject all queued handler requests with the given error.
fn drain_handlers(rx: &mut mpsc::Receiver<HandlerRequest>, err: HandlerDispatchError) {
    while let Ok(req) = rx.try_recv() {
        let _ = req.result_tx.send(HandlerOutcome::DispatchFailed(err.clone()));
    }
}

/// Return a type-appropriate zero/default value for an expose/define field.
pub fn default_value_for_type(ty: &TypeExpr) -> Value {
    match ty {
        TypeExpr::Named(ident, _) => match ident.name.as_str() {
            "int"    => Value::Int(0),
            "double" => Value::Double(0.0),
            "bool"   => Value::Bool(false),
            "String" => Value::Str(String::new()),
            _        => Value::Void,
        },
        _ => Value::Void,
    }
}
