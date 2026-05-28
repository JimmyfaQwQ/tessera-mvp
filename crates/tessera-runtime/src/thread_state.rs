use std::sync::Arc;
use std::sync::atomic::Ordering;
use tokio::sync::{Mutex, RwLock, mpsc, oneshot, watch};
use indexmap::IndexMap;

use crate::{Value, HandlerDispatchError, TesseraFuture, FutureOutcome};
use crate::signal::{BreakablePrimitive, BrokenReason};

pub type ThreadId = u64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThreadStatus {
    Running,
    Terminating,
    Terminated,
    Crashed(String),
}

pub struct HandlerRequest {
    pub handler_name: String,
    pub args: Vec<Value>,
    pub result_tx: oneshot::Sender<HandlerOutcome>,
}

pub enum HandlerOutcome {
    Dispatched(TesseraFuture),
    DispatchFailed(HandlerDispatchError),
}

/// Bundle taken once by the event loop at startup. Always present from `new()`.
pub struct TerminateBundle {
    /// Resolves when `terminate()` is first called.
    pub signal_rx: oneshot::Receiver<()>,
    /// Event loop sends the final thread outcome here.
    pub result_tx: oneshot::Sender<FutureOutcome>,
}

pub struct ThreadState {
    pub id: ThreadId,
    pub template_name: Option<String>,
    /// Whether this thread declared `__on_terminate__` and can be terminated.
    pub is_terminatable: bool,

    status: Mutex<ThreadStatus>,
    status_tx: watch::Sender<ThreadStatus>,

    pub handler_tx: mpsc::Sender<HandlerRequest>,

    pub expose_fields: Arc<RwLock<IndexMap<String, Value>>>,
    pub expose_mutable_fields: Arc<RwLock<IndexMap<String, Value>>>,

    /// Created at `new()`, taken once by the event loop at startup.
    terminate_bundle: Mutex<Option<TerminateBundle>>,
    /// Fired once by the first `terminate()` caller.
    terminate_signal_tx: Mutex<Option<oneshot::Sender<()>>>,
    /// Cached future returned to all `terminate()` callers.
    terminate_future: TesseraFuture,

    pub exclusive_mode: watch::Sender<bool>,

    /// Primitives bound to this thread via `expose` / `expose_mutable`.
    /// Broken when this thread reaches Terminated or Crashed.
    owned_primitives: std::sync::Mutex<Vec<Arc<dyn BreakablePrimitive>>>,
}

impl std::fmt::Debug for ThreadState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThreadState")
            .field("id", &self.id)
            .field("template_name", &self.template_name)
            .finish()
    }
}

static THREAD_ID_COUNTER: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

impl ThreadState {
    pub fn new(
        template_name: Option<String>,
        handler_tx: mpsc::Sender<HandlerRequest>,
        is_terminatable: bool,
    ) -> Arc<Self> {
        let id = THREAD_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
        let (status_tx, _status_rx) = watch::channel(ThreadStatus::Running);
        let (signal_tx, signal_rx) = oneshot::channel::<()>();
        let (result_tx, result_rx) = oneshot::channel::<FutureOutcome>();
        let terminate_future = TesseraFuture::new(result_rx);

        Arc::new(Self {
            id,
            template_name,
            is_terminatable,
            status: Mutex::new(ThreadStatus::Running),
            status_tx,
            handler_tx,
            expose_fields: Arc::new(RwLock::new(IndexMap::new())),
            expose_mutable_fields: Arc::new(RwLock::new(IndexMap::new())),
            terminate_bundle: Mutex::new(Some(TerminateBundle { signal_rx, result_tx })),
            terminate_signal_tx: Mutex::new(Some(signal_tx)),
            terminate_future,
            exclusive_mode: watch::channel(false).0,
            owned_primitives: std::sync::Mutex::new(Vec::new()),
        })
    }

    pub async fn status(&self) -> ThreadStatus {
        self.status.lock().await.clone()
    }

    pub async fn set_status(&self, s: ThreadStatus) {
        *self.status.lock().await = s.clone();
        let _ = self.status_tx.send(s.clone());
        // When the thread fully stops, break all primitives it owns so that
        // waiters on those primitives receive a failure notification.
        let reason = match &s {
            ThreadStatus::Terminated  => Some(BrokenReason::OwnerGone),
            ThreadStatus::Crashed(_)  => Some(BrokenReason::OwnerCrashed),
            _ => None,
        };
        if let Some(r) = reason {
            let prims = self.owned_primitives.lock().unwrap();
            for p in &*prims {
                p.break_with(r.clone());
            }
        }
    }

    /// Request termination. All callers receive the same cached Future<void>.
    pub async fn terminate(&self) -> TesseraFuture {
        if let Some(tx) = self.terminate_signal_tx.lock().await.take() {
            let _ = tx.send(());
        }
        self.terminate_future.clone()
    }

    /// Called once by the event loop at startup to claim the terminate channels.
    ///
    /// Returns `None` only if it has already been taken — calling code must
    /// ensure this is invoked exactly once per `ThreadState`. Use
    /// [`Self::take_terminate_bundle_or_panic`] when the caller is the
    /// unique owner (the event loop) and a second take indicates a bug.
    pub async fn take_terminate_bundle(&self) -> Option<TerminateBundle> {
        self.terminate_bundle.lock().await.take()
    }

    /// Claim the terminate bundle, panicking if it has already been taken.
    ///
    /// # Invariant
    ///
    /// `ThreadState::new` always installs the bundle, and only the event loop
    /// for this thread is allowed to claim it (exactly once, before entering
    /// the main loop body). A second call indicates the event loop was invoked
    /// twice for the same `ThreadState`, which is a bug — fail loudly.
    pub async fn take_terminate_bundle_or_panic(&self) -> TerminateBundle {
        self.terminate_bundle
            .lock()
            .await
            .take()
            .expect("terminate bundle already taken — event loop invoked twice on this ThreadState")
    }

    /// Dispatch a handler call. Returns a Future the caller can await.
    pub async fn dispatch_handler(
        &self,
        handler_name: String,
        args: Vec<Value>,
    ) -> Result<TesseraFuture, HandlerDispatchError> {
        let status = self.status().await;
        match status {
            ThreadStatus::Terminated  => return Err(HandlerDispatchError::TargetTerminated),
            ThreadStatus::Terminating => return Err(HandlerDispatchError::TargetTerminating),
            ThreadStatus::Crashed(_)  => return Err(HandlerDispatchError::TargetCrashed),
            ThreadStatus::Running => {}
        }

        let (tx, rx) = oneshot::channel();
        let req = HandlerRequest { handler_name, args, result_tx: tx };

        if self.handler_tx.send(req).await.is_err() {
            return Err(HandlerDispatchError::TargetCrashed);
        }

        match rx.await {
            Ok(HandlerOutcome::Dispatched(fut)) => Ok(fut),
            Ok(HandlerOutcome::DispatchFailed(e)) => Err(e),
            Err(_) => Err(HandlerDispatchError::TargetCrashed),
        }
    }

    /// Register a primitive as owned by this thread. Called by `maybe_sync_expose`
    /// when a primitive is first exposed by this thread. Breaking primitives happens
    /// in `set_status(Terminated/Crashed)`.
    pub fn register_owned(&self, prim: Arc<dyn BreakablePrimitive>) {
        self.owned_primitives.lock().unwrap().push(prim);
    }

    pub fn exclusive_mode(&self) -> bool {
        *self.exclusive_mode.borrow()
    }

    pub fn set_exclusive(&self, v: bool) {
        let _ = self.exclusive_mode.send(v);
    }

    /// Subscribe to exclusive-mode changes. Use `rx.wait_for(|&v| !v).await`
    /// to block without busy-waiting until exclusive mode ends.
    pub fn subscribe_exclusive(&self) -> watch::Receiver<bool> {
        self.exclusive_mode.subscribe()
    }
}
