use async_channel::{Sender, Receiver, unbounded};
use crate::Value;

/// Thread-safe FIFO queue for Tessera's Queue<T>.
#[derive(Debug)]
pub struct TesseraQueue {
    tx: Sender<Value>,
    rx: Receiver<Value>,
}

impl TesseraQueue {
    pub fn new() -> Self {
        let (tx, rx) = unbounded();
        Self { tx, rx }
    }

    /// Push without blocking. Returns false if the queue is closed.
    pub fn try_push(&self, value: Value) -> bool {
        self.tx.try_send(value).is_ok()
    }

    /// Non-blocking pop. Returns None if empty or closed.
    pub fn try_pop(&self) -> Option<Value> {
        self.rx.try_recv().ok()
    }

    /// Async push. Returns false if the queue is closed.
    pub async fn enqueue(&self, value: Value) -> bool {
        self.tx.send(value).await.is_ok()
    }

    /// Async pop. Returns None if queue is closed and empty.
    pub async fn dequeue(&self) -> Option<Value> {
        self.rx.recv().await.ok()
    }

    pub fn close(&self) {
        self.tx.close();
    }

    pub fn len(&self) -> usize {
        self.rx.len()
    }
}
