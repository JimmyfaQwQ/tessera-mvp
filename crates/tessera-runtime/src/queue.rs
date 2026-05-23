use async_channel::{Sender, Receiver};
use crate::Value;

/// Error returned by the synchronous `push()` method.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueuePushError {
    /// Bounded queue is at capacity.
    Full,
    /// The queue has been closed.
    Closed,
}

impl std::fmt::Display for QueuePushError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QueuePushError::Full   => write!(f, "Full"),
            QueuePushError::Closed => write!(f, "Closed"),
        }
    }
}

/// Thread-safe FIFO queue for Tessera's `Queue<T>`.
///
/// - `capacity <= 0` → unbounded; `capacity > 0` → bounded with that limit.
#[derive(Debug)]
pub struct TesseraQueue {
    tx: Sender<Value>,
    rx: Receiver<Value>,
}

impl TesseraQueue {
    pub fn new(capacity: i32) -> Self {
        let (tx, rx) = if capacity > 0 {
            async_channel::bounded(capacity as usize)
        } else {
            async_channel::unbounded()
        };
        Self { tx, rx }
    }

    // ── Synchronous (non-blocking) interface ─────────────────────────────────

    /// Sync push returning explicit failure via `Result`.
    /// - Unbounded: always `Ok(())` unless closed.
    /// - Bounded full: `Err(Full)`.
    /// - Closed: `Err(Closed)`.
    pub fn push(&self, value: Value) -> Result<(), QueuePushError> {
        match self.tx.try_send(value) {
            Ok(())                                      => Ok(()),
            Err(async_channel::TrySendError::Full(_))   => Err(QueuePushError::Full),
            Err(async_channel::TrySendError::Closed(_)) => Err(QueuePushError::Closed),
        }
    }

    /// Non-blocking push; `false` if full or closed.
    pub fn try_push(&self, value: Value) -> bool {
        self.tx.try_send(value).is_ok()
    }

    /// Non-blocking pop; `None` if empty or closed.
    pub fn try_pop(&self) -> Option<Value> {
        self.rx.try_recv().ok()
    }

    /// Atomic snapshot of current element count.
    pub fn size(&self) -> usize {
        self.rx.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rx.is_empty()
    }

    pub fn is_closed(&self) -> bool {
        self.tx.is_closed()
    }

    /// Close the queue; subsequent pushes/enqueues are rejected.
    pub fn close(&self) {
        self.tx.close();
    }

    // ── Asynchronous interface ────────────────────────────────────────────────

    /// Async push; waits for space (bounded) or queues immediately (unbounded).
    /// Returns `false` if the queue is already closed.
    pub async fn enqueue(&self, value: Value) -> bool {
        self.tx.send(value).await.is_ok()
    }

    /// Async pop; waits until an element is available or the queue is closed+empty.
    /// Returns `None` when closed and drained.
    pub async fn dequeue(&self) -> Option<Value> {
        self.rx.recv().await.ok()
    }

    /// Suspend until the queue is non-empty or closed.
    pub async fn wait_for_non_empty(&self) {
        loop {
            if !self.rx.is_empty() || self.rx.is_closed() {
                return;
            }
            tokio::task::yield_now().await;
        }
    }
}
