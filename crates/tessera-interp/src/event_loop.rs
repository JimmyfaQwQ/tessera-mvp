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

// `let _ = …_tx.send(…)` appears throughout this file on `ready_tx` and
// `result_tx`. Both senders are oneshot channels whose receivers may legitimately
// be dropped (fire-and-forget thread spawns, parents that gave up waiting). A
// failed send means "the listener is gone" — no action to take, no log to emit.
// The same applies to `req.result_tx.send(...)` inside `drain_handlers`: the
// dispatcher has already moved on, and a missed send simply degrades a precise
// `TargetTerminating` into a generic `TargetCrashed` (via the oneshot::Receiver
// in `ThreadState::dispatch_handler`).
pub async fn run_thread_task(
    interp: Interpreter,
    decl: Option<Arc<ThreadTemplateDecl>>,
    args: Vec<Value>,
    body: Arc<Block>,
    state: Arc<ThreadState>,
    mut handler_rx: mpsc::Receiver<HandlerRequest>,
    ready_tx: tokio::sync::oneshot::Sender<()>,
) {
    // ── Setup ────────────────────────────────────────────────────────────────

    *interp.0.current_thread_state.borrow_mut() = Some(state.clone());

    if let Some(d) = &decl {
        // Register member functions into func_table so the thread body can call them.
        // First mutation on a thread-local interp state pays the Rc clone via make_mut.
        {
            let mut table = interp.0.func_table.borrow_mut();
            let table = std::rc::Rc::make_mut(&mut table);
            for m in &d.members {
                let func: Option<&FuncDef> = match m {
                    MemberFunc(f) | OnEnter(f) | OnExit(f) | OnTerminate(f) => Some(f),
                    _ => None,
                };
                if let Some(f) = func {
                    table.insert(f.name.name.clone(), Arc::new(f.clone()));
                }
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
                            // A field initializer that fails is a real error: crash
                            // the thread instead of silently substituting a default.
                            // Dropping `ready_tx` (by returning) unblocks the parent's
                            // `ready_rx.await`, which ignores the receive error.
                            Err(err) => {
                                state.set_status(ThreadStatus::Crashed(err.to_string())).await;
                                drain_handlers(&mut handler_rx, HandlerDispatchError::TargetCrashed);
                                if let Some(b) = state.take_terminate_bundle().await {
                                    let _ = b.result_tx.send(FutureOutcome::Failed(err.to_string()));
                                }
                                return;
                            }
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
        // Arity mismatch here means the type checker let through a malformed
        // call: crash the thread rather than silently dropping or zero-filling.
        if d.params.len() != args.len() {
            let template_name = d.name.as_ref().map(|i| i.name.as_str()).unwrap_or("<anonymous>");
            let msg = format!(
                "thread template '{}' expects {} argument(s), got {}",
                template_name, d.params.len(), args.len(),
            );
            state.set_status(ThreadStatus::Crashed(msg.clone())).await;
            drain_handlers(&mut handler_rx, HandlerDispatchError::TargetCrashed);
            if let Some(b) = state.take_terminate_bundle().await {
                let _ = b.result_tx.send(FutureOutcome::Failed(msg));
            }
            return;
        }
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
                // Notify parent that __on_enter__ is done (failed), then crash.
                let _ = ready_tx.send(());
                state.set_status(ThreadStatus::Crashed(e.to_string())).await;
                drain_handlers(&mut handler_rx, HandlerDispatchError::TargetCrashed);
                if let Some(b) = state.take_terminate_bundle().await {
                    let _ = b.result_tx.send(FutureOutcome::Failed(e.to_string()));
                }
                return;
            }
        }
    }

    // Notify parent that __on_enter__ has finished; parent can now proceed.
    let _ = ready_tx.send(());

    // ── Claim terminate channels (always present from ThreadState::new) ──────

    let TerminateBundle { signal_rx: mut terminate_rx, result_tx } =
        state.take_terminate_bundle_or_panic().await;
    let mut result_tx = Some(result_tx);

    // ── Main event loop ──────────────────────────────────────────────────────

    // Clone interp so the body future and handler dispatch use independent
    // Rust borrows while sharing the same Rc<InterpState>.
    let body_interp = interp.clone();
    let mut body_fut = body_interp.exec_block(&body);

    // R-EXCL-3: tracks whether terminate() arrived while exclusive_mode was
    // active.  When true the state is already Terminating, the handler queue
    // has been drained, but teardown hooks are deferred until the exclusive
    // block exits.
    let mut terminate_during_exclusive = false;

    // R-HANDLER-2: subscribe to the handler-in-flight watch so the select loop
    // wakes when the running handler task releases the gate; without this the
    // loop could sleep on body_fut forever after a handler completes, leaving
    // the next queued handler waiting in mpsc.
    let mut hf_changed = state.subscribe_handler_in_flight();

    loop {
        let exclusive = state.exclusive_mode();
        let handler_busy = state.handler_in_flight();

        // R-EXCL-3: exclusive block just ended and a deferred terminate is
        // pending — run teardown now (body_fut is abandoned).
        if terminate_during_exclusive && !exclusive {
            match run_teardown_hooks(&interp, decl.as_deref(), true).await {
                Ok(()) => {
                    state.set_status(ThreadStatus::Terminated).await;
                    if let Some(tx) = result_tx.take() {
                        let _ = tx.send(FutureOutcome::Ok(Value::Void));
                    }
                }
                Err(e) => {
                    state.set_status(ThreadStatus::Crashed(e.to_string())).await;
                    if let Some(tx) = result_tx.take() {
                        let _ = tx.send(FutureOutcome::Failed(e.to_string()));
                    }
                }
            }
            break;
        }

        tokio::select! {
            biased;

            // ── Terminate signal (highest priority) ──────────────────────────
            //
            // No longer gated on `!exclusive`.  When terminate() arrives while
            // the thread is inside an #exclusive block (R-EXCL-3):
            //   • state transitions to Terminating immediately so that callers
            //     of dispatch_handler() see TargetTerminating right away;
            //   • the handler queue is drained;
            //   • teardown hooks are deferred until the exclusive block exits
            //     (handled by the `terminate_during_exclusive` check above).
            // Outside exclusive mode the original behaviour is preserved.
            _ = &mut terminate_rx, if !terminate_during_exclusive => {
                state.set_status(ThreadStatus::Terminating).await;
                drain_handlers(&mut handler_rx, HandlerDispatchError::TargetTerminating);
                if exclusive {
                    // Defer teardown until exclusive block exits.
                    terminate_during_exclusive = true;
                } else {
                    match run_teardown_hooks(&interp, decl.as_deref(), true).await {
                        Ok(()) => {
                            state.set_status(ThreadStatus::Terminated).await;
                            if let Some(tx) = result_tx.take() {
                                let _ = tx.send(FutureOutcome::Ok(Value::Void));
                            }
                        }
                        Err(e) => {
                            state.set_status(ThreadStatus::Crashed(e.to_string())).await;
                            if let Some(tx) = result_tx.take() {
                                let _ = tx.send(FutureOutcome::Failed(e.to_string()));
                            }
                        }
                    }
                    // body_fut is dropped here, abandoning the main body
                    break;
                }
            }

            // ── Main body ────────────────────────────────────────────────────
            result = body_fut.as_mut() => {
                match result {
                    Ok(_) => {
                        if terminate_during_exclusive {
                            // Body completed after the exclusive block ended with
                            // a deferred terminate pending — run teardown, not the
                            // normal __on_exit__ path.
                            match run_teardown_hooks(&interp, decl.as_deref(), true).await {
                                Ok(()) => {
                                    state.set_status(ThreadStatus::Terminated).await;
                                    if let Some(tx) = result_tx.take() {
                                        let _ = tx.send(FutureOutcome::Ok(Value::Void));
                                    }
                                }
                                Err(e) => {
                                    state.set_status(ThreadStatus::Crashed(e.to_string())).await;
                                    if let Some(tx) = result_tx.take() {
                                        let _ = tx.send(FutureOutcome::Failed(e.to_string()));
                                    }
                                }
                            }
                        } else {
                            let hook_result = run_hook(&interp, decl.as_deref(), "__on_exit__").await;
                            match hook_result {
                                Ok(()) => {
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
                        }
                    }
                    Err(e) => {
                        // If terminate() was already handled (either normally or
                        // deferred via exclusive), honour it over the crash.
                        if terminate_during_exclusive || terminate_rx.try_recv().is_ok() {
                            state.set_status(ThreadStatus::Terminating).await;
                            drain_handlers(&mut handler_rx, HandlerDispatchError::TargetTerminating);
                            match run_teardown_hooks(&interp, decl.as_deref(), true).await {
                                Ok(()) => {
                                    state.set_status(ThreadStatus::Terminated).await;
                                    if let Some(tx) = result_tx.take() {
                                        let _ = tx.send(FutureOutcome::Ok(Value::Void));
                                    }
                                }
                                Err(he) => {
                                    state.set_status(ThreadStatus::Crashed(he.to_string())).await;
                                    if let Some(tx) = result_tx.take() {
                                        let _ = tx.send(FutureOutcome::Failed(he.to_string()));
                                    }
                                }
                            }
                        } else {
                            state.set_status(ThreadStatus::Crashed(e.to_string())).await;
                            drain_handlers(&mut handler_rx, HandlerDispatchError::TargetCrashed);
                            if let Some(tx) = result_tx.take() {
                                let _ = tx.send(FutureOutcome::Failed(e.to_string()));
                            }
                        }
                    }
                }
                break;
            }

            // ── Handler dispatch (inline, cooperative) ───────────────────────
            // R-HANDLER-2: gate on `!handler_busy` so a second handler cannot
            // start while another is still in flight on this thread.
            req = handler_rx.recv(), if !exclusive && !terminate_during_exclusive && !handler_busy => {
                if let Some(req) = req {
                    dispatch_handler_inline(&interp, decl.as_deref(), req, state.clone());
                }
            }

            // ── Wakeup branch for handler-in-flight transitions ──────────────
            // No-op body: the loop re-iterates and the handler_rx gate above
            // re-evaluates. Without this branch a paused select would sleep on
            // body_fut and never notice that the previous handler completed.
            _ = hf_changed.changed(), if handler_busy => {}
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
///
/// R-EXCL-1: the spawned task defers execution until exclusive_mode is cleared
/// so that handlers dispatched just before an #exclusive block cannot interleave
/// with it at yield points inside the block.
fn dispatch_handler_inline(
    interp: &Interpreter,
    decl: Option<&ThreadTemplateDecl>,
    req: HandlerRequest,
    state: Arc<ThreadState>,
) {
    let handler = decl.and_then(|d| find_handler(d, &req.handler_name));

    match handler {
        Some(h) => {
            // R-HANDLER-2: claim the handler-in-flight gate synchronously so
            // the main select gate already sees `handler_busy=true` on its very
            // next iteration. The spawned task releases the gate on both the
            // success and failure paths before sending the outcome.
            state.set_handler_in_flight(true);

            let (exec_tx, exec_rx) = oneshot::channel::<FutureOutcome>();
            let _ = req.result_tx.send(HandlerOutcome::Dispatched(TesseraFuture::new(exec_rx)));

            let h = h.clone();
            let interp = interp.clone();
            let args = req.args;
            let state_for_task = state.clone();
            tokio::task::spawn_local(async move {
                // R-EXCL-1: wait (without busy-polling) for any in-progress
                // exclusive block on this thread to end before starting handler
                // execution, so handlers dispatched just before the block cannot
                // interleave with it at await points inside the block.
                if state_for_task.exclusive_mode() {
                    let mut rx = state_for_task.subscribe_exclusive();
                    // wait_for checks the current value first, so this is
                    // race-free: if exclusive already ended we return immediately.
                    let _ = rx.wait_for(|&v| !v).await;
                }
                let outcome = match interp.exec_handler_body(&h, args).await {
                    Ok(v)  => FutureOutcome::Ok(v),
                    Err(e) => FutureOutcome::Failed(e.to_string()),
                };
                // Release the gate BEFORE delivering the outcome so the caller
                // is unblocked simultaneously with the main loop becoming free
                // to accept the next handler.
                state_for_task.set_handler_in_flight(false);
                let _ = exec_tx.send(outcome);
            });
        }
        None => {
            let _ = req.result_tx.send(HandlerOutcome::DispatchFailed(
                HandlerDispatchError::TargetCrashed,
            ));
        }
    }
}

/// Run a named lifecycle hook if present, propagating any error it raises so
/// the caller can surface it (previously these errors were silently dropped).
async fn run_hook(
    interp: &Interpreter,
    decl: Option<&ThreadTemplateDecl>,
    name: &str,
) -> Result<(), tessera_runtime::RuntimeError> {
    if let Some(d) = decl {
        if let Some(hook) = find_thread_hook(d, name) {
            let hook = hook.clone();
            interp.exec_func_def_body(&hook, vec![]).await?;
        }
    }
    Ok(())
}

/// Run teardown hooks (`__on_terminate__` optionally, then `__on_exit__`). Both
/// always run so cleanup is not skipped, but the first error is returned so the
/// caller can mark the thread crashed instead of silently dropping it.
async fn run_teardown_hooks(
    interp: &Interpreter,
    decl: Option<&ThreadTemplateDecl>,
    run_terminate: bool,
) -> Result<(), tessera_runtime::RuntimeError> {
    let mut first_err = None;
    if run_terminate {
        if let Err(e) = run_hook(interp, decl, "__on_terminate__").await {
            first_err.get_or_insert(e);
        }
    }
    if let Err(e) = run_hook(interp, decl, "__on_exit__").await {
        first_err.get_or_insert(e);
    }
    match first_err {
        Some(e) => Err(e),
        None => Ok(()),
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
