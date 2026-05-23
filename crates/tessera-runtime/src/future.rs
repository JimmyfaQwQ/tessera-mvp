use std::sync::Arc;
use tokio::sync::{oneshot, Mutex};
use crate::Value;
use crate::error::HandlerDispatchError;

#[derive(Debug, Clone)]
pub enum FutureOutcome {
    Ok(Value),
    Failed(String),
}

/// Tessera Future<T>: async result that can be awaited or .wait()-ed.
#[derive(Debug, Clone)]
pub struct TesseraFuture {
    inner: Arc<Mutex<FutureState>>,
}

#[derive(Debug)]
enum FutureState {
    Pending(Option<oneshot::Receiver<FutureOutcome>>),
    Resolved(FutureOutcome),
}

impl TesseraFuture {
    pub fn new(rx: oneshot::Receiver<FutureOutcome>) -> Self {
        Self { inner: Arc::new(Mutex::new(FutureState::Pending(Some(rx)))) }
    }

    pub fn immediate(outcome: FutureOutcome) -> Self {
        Self { inner: Arc::new(Mutex::new(FutureState::Resolved(outcome))) }
    }

    /// Non-blocking check: true if the future has already completed (Ok or Failed).
    pub fn is_done(&self) -> bool {
        match self.inner.try_lock() {
            Ok(guard) => matches!(*guard, FutureState::Resolved(_)),
            Err(_) => false, // locked = currently being resolved, treat as pending
        }
    }

    pub async fn resolve(&self) -> FutureOutcome {
        let mut guard = self.inner.lock().await;
        match &mut *guard {
            FutureState::Resolved(o) => o.clone(),
            FutureState::Pending(rx_opt) => {
                let rx = rx_opt.take().expect("Future polled twice");
                let outcome = rx.await.unwrap_or(FutureOutcome::Failed("sender dropped".into()));
                *guard = FutureState::Resolved(outcome.clone());
                outcome
            }
        }
    }
}

/// Tessera HandlerFuture<R>: result of dispatching a handler.
/// `.waitHandler()` / `.awaitHandler()` returns Result<Value, HandlerDispatchError>.
#[derive(Debug, Clone)]
pub struct TesseraHandlerFuture {
    inner: Arc<Mutex<HandlerFutureState>>,
}

#[derive(Debug)]
pub enum HandlerFutureState {
    /// Waiting for dispatch result
    Pending(Option<oneshot::Receiver<HandlerFutureOutcome>>),
    /// Dispatch failed (thread not available)
    DispatchFailed(HandlerDispatchError),
    /// Dispatch already succeeded; waiting on the execution future
    AlreadyDispatched(Option<TesseraFuture>),
    /// Dispatch accepted; waiting for execution result
    Executing(Option<oneshot::Receiver<FutureOutcome>>),
    /// Execution succeeded
    Done(Value),
    /// Execution failed (handler panicked)
    ExecutionFailed(String),
}

#[derive(Debug)]
pub enum HandlerFutureOutcome {
    Accepted(oneshot::Receiver<FutureOutcome>),
    Rejected(HandlerDispatchError),
}

/// Result of resolving a HandlerFuture.
/// Distinct from HandlerDispatchError so the eval layer can propagate execution
/// failures as RuntimeError::Panic (spec §3.4.2) vs dispatch failures as Err.
#[derive(Debug)]
pub enum HandlerResolveResult {
    /// Handler succeeded; holds the return value.
    Ok(Value),
    /// Dispatch failed before execution started (caller can handle via isErr).
    DispatchFailed(HandlerDispatchError),
    /// Handler was dispatched but crashed during execution (caller must crash too).
    ExecutionFailed(String),
}

impl TesseraHandlerFuture {
    pub fn new(rx: oneshot::Receiver<HandlerFutureOutcome>) -> Self {
        Self { inner: Arc::new(Mutex::new(HandlerFutureState::Pending(Some(rx)))) }
    }

    pub fn rejected(err: HandlerDispatchError) -> Self {
        Self { inner: Arc::new(Mutex::new(HandlerFutureState::DispatchFailed(err))) }
    }

    /// Create a HandlerFuture that is already past the dispatch phase.
    /// The caller awaits the given `TesseraFuture` for the execution result.
    pub fn from_future(fut: TesseraFuture) -> Self {
        Self { inner: Arc::new(Mutex::new(HandlerFutureState::AlreadyDispatched(Some(fut)))) }
    }

    /// Non-blocking: true if in any terminal state (Done, DispatchFailed, ExecutionFailed).
    pub fn is_done(&self) -> bool {
        match self.inner.try_lock() {
            Ok(guard) => matches!(
                *guard,
                HandlerFutureState::Done(_)
                    | HandlerFutureState::DispatchFailed(_)
                    | HandlerFutureState::ExecutionFailed(_)
            ),
            Err(_) => false,
        }
    }

    /// Non-blocking: true only if terminal state is Done(Ok).
    /// Returns false when pending OR when completed with any failure.
    pub fn is_ok(&self) -> bool {
        match self.inner.try_lock() {
            Ok(guard) => matches!(*guard, HandlerFutureState::Done(_)),
            Err(_) => false,
        }
    }

    /// Non-blocking: true if terminal state is any failure (DispatchFailed or ExecutionFailed).
    pub fn is_err(&self) -> bool {
        match self.inner.try_lock() {
            Ok(guard) => matches!(
                *guard,
                HandlerFutureState::DispatchFailed(_) | HandlerFutureState::ExecutionFailed(_)
            ),
            Err(_) => false,
        }
    }

    /// Non-blocking: returns the error message if in a failure terminal state.
    pub fn get_err(&self) -> Option<String> {
        match self.inner.try_lock() {
            Ok(guard) => match &*guard {
                HandlerFutureState::DispatchFailed(e) => Some(e.to_string()),
                HandlerFutureState::ExecutionFailed(msg) => Some(msg.clone()),
                _ => None,
            },
            Err(_) => None,
        }
    }

    /// Resolve the handler future.
    /// Returns HandlerResolveResult to distinguish dispatch failure (caller handles)
    /// from execution failure (caller must crash, per spec §3.4.2).
    pub async fn resolve(&self) -> HandlerResolveResult {
        let mut guard = self.inner.lock().await;
        loop {
            match &mut *guard {
                HandlerFutureState::DispatchFailed(e) => return HandlerResolveResult::DispatchFailed(e.clone()),
                HandlerFutureState::Done(v) => return HandlerResolveResult::Ok(v.clone()),
                HandlerFutureState::ExecutionFailed(msg) => {
                    return HandlerResolveResult::ExecutionFailed(msg.clone());
                }
                HandlerFutureState::AlreadyDispatched(fut_opt) => {
                    let fut = fut_opt.take().expect("HandlerFuture polled twice");
                    match fut.resolve().await {
                        FutureOutcome::Ok(v) => {
                            *guard = HandlerFutureState::Done(v.clone());
                            return HandlerResolveResult::Ok(v);
                        }
                        FutureOutcome::Failed(msg) => {
                            *guard = HandlerFutureState::ExecutionFailed(msg.clone());
                            return HandlerResolveResult::ExecutionFailed(msg);
                        }
                    }
                }
                HandlerFutureState::Pending(rx_opt) => {
                    let rx = rx_opt.take().expect("HandlerFuture polled twice");
                    match rx.await {
                        Ok(HandlerFutureOutcome::Accepted(exec_rx)) => {
                            *guard = HandlerFutureState::Executing(Some(exec_rx));
                        }
                        Ok(HandlerFutureOutcome::Rejected(e)) => {
                            *guard = HandlerFutureState::DispatchFailed(e.clone());
                            return HandlerResolveResult::DispatchFailed(e);
                        }
                        Err(_) => {
                            let e = HandlerDispatchError::TargetCrashed;
                            *guard = HandlerFutureState::DispatchFailed(e.clone());
                            return HandlerResolveResult::DispatchFailed(e);
                        }
                    }
                }
                HandlerFutureState::Executing(rx_opt) => {
                    let rx = rx_opt.take().expect("HandlerFuture exec polled twice");
                    match rx.await {
                        Ok(FutureOutcome::Ok(v)) => {
                            *guard = HandlerFutureState::Done(v.clone());
                            return HandlerResolveResult::Ok(v);
                        }
                        Ok(FutureOutcome::Failed(msg)) => {
                            *guard = HandlerFutureState::ExecutionFailed(msg.clone());
                            return HandlerResolveResult::ExecutionFailed(msg);
                        }
                        Err(_) => {
                            let msg = "sender dropped".to_string();
                            *guard = HandlerFutureState::ExecutionFailed(msg.clone());
                            return HandlerResolveResult::ExecutionFailed(msg);
                        }
                    }
                }
            }
        }
    }
}
