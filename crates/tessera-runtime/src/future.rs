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

    /// Resolve the handler future, returning Result<Value, HandlerDispatchError>.
    /// ExecutionFailed returns Err with a special sentinel.
    pub async fn resolve(&self) -> Result<Value, HandlerDispatchError> {
        let mut guard = self.inner.lock().await;
        loop {
            match &mut *guard {
                HandlerFutureState::DispatchFailed(e) => return Err(e.clone()),
                HandlerFutureState::Done(v) => return Ok(v.clone()),
                HandlerFutureState::ExecutionFailed(_msg) => {
                    // Spec: interacting with execution-failed HandlerFuture crashes caller.
                    // Return a special error that the interpreter turns into a thread crash.
                    return Err(HandlerDispatchError::TargetCrashed); // repurposed
                }
                HandlerFutureState::AlreadyDispatched(fut_opt) => {
                    let fut = fut_opt.take().expect("HandlerFuture polled twice");
                    match fut.resolve().await {
                        FutureOutcome::Ok(v) => {
                            *guard = HandlerFutureState::Done(v.clone());
                            return Ok(v);
                        }
                        FutureOutcome::Failed(msg) => {
                            *guard = HandlerFutureState::ExecutionFailed(msg);
                            // loop continues to ExecutionFailed arm
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
                            return Err(e);
                        }
                        Err(_) => {
                            *guard = HandlerFutureState::DispatchFailed(HandlerDispatchError::TargetCrashed);
                            return Err(HandlerDispatchError::TargetCrashed);
                        }
                    }
                }
                HandlerFutureState::Executing(rx_opt) => {
                    let rx = rx_opt.take().expect("HandlerFuture exec polled twice");
                    match rx.await {
                        Ok(FutureOutcome::Ok(v)) => {
                            *guard = HandlerFutureState::Done(v.clone());
                            return Ok(v);
                        }
                        Ok(FutureOutcome::Failed(msg)) => {
                            *guard = HandlerFutureState::ExecutionFailed(msg);
                        }
                        Err(_) => {
                            *guard = HandlerFutureState::ExecutionFailed("sender dropped".into());
                        }
                    }
                }
            }
        }
    }
}
