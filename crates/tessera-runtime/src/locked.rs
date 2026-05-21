use tokio::sync::Mutex;
use crate::Value;

pub struct TesseraLocked {
    inner: Mutex<Value>,
}

impl std::fmt::Debug for TesseraLocked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("locked<T>")
    }
}

impl TesseraLocked {
    pub fn new(initial: Value) -> Self {
        Self { inner: Mutex::new(initial) }
    }

    pub async fn get(&self) -> Value {
        self.inner.lock().await.clone()
    }

    pub async fn set(&self, value: Value) {
        *self.inner.lock().await = value;
    }

    /// Locks and calls `f` with mutable access. Returns the result.
    pub async fn with<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut Value) -> R,
    {
        let mut guard = self.inner.lock().await;
        f(&mut guard)
    }
}
