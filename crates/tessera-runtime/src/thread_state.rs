use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{Mutex, RwLock, mpsc, oneshot, watch};
use indexmap::IndexMap;

use crate::{Value, HandlerDispatchError, TesseraFuture, FutureOutcome};

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

    pub exclusive_mode: AtomicBool,
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
    ) -> Arc<Self> {
        let id = THREAD_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
        let (status_tx, _status_rx) = watch::channel(ThreadStatus::Running);
        let (signal_tx, signal_rx) = oneshot::channel::<()>();
        let (result_tx, result_rx) = oneshot::channel::<FutureOutcome>();
        let terminate_future = TesseraFuture::new(result_rx);

        Arc::new(Self {
            id,
            template_name,
            status: Mutex::new(ThreadStatus::Running),
            status_tx,
            handler_tx,
            expose_fields: Arc::new(RwLock::new(IndexMap::new())),
            expose_mutable_fields: Arc::new(RwLock::new(IndexMap::new())),
            terminate_bundle: Mutex::new(Some(TerminateBundle { signal_rx, result_tx })),
            terminate_signal_tx: Mutex::new(Some(signal_tx)),
            terminate_future,
            exclusive_mode: AtomicBool::new(false),
        })
    }

    pub async fn status(&self) -> ThreadStatus {
        self.status.lock().await.clone()
    }

    pub async fn set_status(&self, s: ThreadStatus) {
        *self.status.lock().await = s.clone();
        let _ = self.status_tx.send(s);
    }

    /// Request termination. All callers receive the same cached Future<void>.
    pub async fn terminate(&self) -> TesseraFuture {
        if let Some(tx) = self.terminate_signal_tx.lock().await.take() {
            let _ = tx.send(());
        }
        self.terminate_future.clone()
    }

    /// Called once by the event loop at startup to claim the terminate channels.
    pub async fn take_terminate_bundle(&self) -> Option<TerminateBundle> {
        self.terminate_bundle.lock().await.take()
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

    pub fn exclusive_mode(&self) -> bool {
        self.exclusive_mode.load(Ordering::Relaxed)
    }

    pub fn set_exclusive(&self, v: bool) {
        self.exclusive_mode.store(v, Ordering::Relaxed);
    }
}
