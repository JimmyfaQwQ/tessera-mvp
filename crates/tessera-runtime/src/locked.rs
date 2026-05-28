use std::sync::Mutex;
use tokio::sync::Notify;
use crate::Value;
use crate::thread_state::ThreadId;

/// `locked<T>` — concurrent-safe shared value.
///
/// Provides two locking layers:
///
/// **Explicit (sync) interface** — `lock` / `tryLock` / `unlock` / `isLocked`:
///   Used when a single Tessera thread needs to hold the lock across multiple
///   operations. The `owner_id` is the unique `ThreadState::id` of the holding
///   Tessera thread.
///
/// **Implicit (async) interface** — `get` / `set`:
///   Convenience methods that wait until no explicit lock is held, then
///   atomically read or write the value and return. Callers need not
///   pre-acquire the explicit lock.
pub struct TesseraLocked {
    inner: Mutex<LockedInner>,
    /// Wakes all waiters whenever the explicit lock is released.
    notify: Notify,
}

struct LockedInner {
    value: Value,
    /// `Some(owner_id)` while explicitly locked; `None` when free.
    owner: Option<ThreadId>,
}

impl std::fmt::Debug for TesseraLocked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let locked = self.inner.lock().map(|g| g.owner.is_some()).unwrap_or(false);
        write!(f, "locked<T>({})", if locked { "locked" } else { "unlocked" })
    }
}

impl TesseraLocked {
    pub fn new(initial: Value) -> Self {
        Self {
            inner: Mutex::new(LockedInner { value: initial, owner: None }),
            notify: Notify::new(),
        }
    }

    // ── Explicit lock interface ───────────────────────────────────────────────

    /// Acquire the explicit lock for `owner_id`.
    /// Suspends (async) until the lock is free.
    /// Returns `Err(())` if `owner_id` already holds the lock (reentrance).
    pub async fn lock(&self, owner_id: ThreadId) -> Result<(), ()> {
        loop {
            {
                let mut g = self.inner.lock().unwrap();
                match g.owner {
                    Some(id) if id == owner_id => return Err(()), // reentrance
                    None => { g.owner = Some(owner_id); return Ok(()); }
                    _ => {} // held by another; fall through to await
                }
            }
            self.notify.notified().await;
        }
    }

    /// Non-blocking attempt to acquire the explicit lock.
    /// Returns `Err(())` on reentrance, `Ok(true)` if acquired, `Ok(false)` if held by another.
    #[allow(clippy::result_unit_err)]
    pub fn try_lock(&self, owner_id: ThreadId) -> Result<bool, ()> {
        let mut g = self.inner.lock().unwrap();
        match g.owner {
            Some(id) if id == owner_id => Err(()),
            None => { g.owner = Some(owner_id); Ok(true) }
            _ => Ok(false),
        }
    }

    /// Release the explicit lock.
    /// Returns `false` if `owner_id` does not currently own it.
    pub fn unlock(&self, owner_id: ThreadId) -> bool {
        let mut g = self.inner.lock().unwrap();
        if g.owner == Some(owner_id) {
            g.owner = None;
            self.notify.notify_waiters();
            true
        } else {
            false
        }
    }

    /// Snapshot: `true` if any thread holds the explicit lock.
    pub fn is_locked(&self) -> bool {
        self.inner.lock().unwrap().owner.is_some()
    }

    // ── Implicit async read/write interface ──────────────────────────────────

    /// Read the value; waits until no explicit lock is held, then returns a clone.
    ///
    /// Because the interpreter runs on a single-OS-thread `LocalSet` (cooperative
    /// scheduling), the check-and-read between `owner.is_none()` and the return
    /// is never interrupted by another task — no separate "data lock" is needed.
    pub async fn get(&self) -> Value {
        loop {
            {
                let g = self.inner.lock().unwrap();
                if g.owner.is_none() {
                    return g.value.clone();
                }
            }
            self.notify.notified().await;
        }
    }

    /// Write the value; waits until no explicit lock is held, then overwrites.
    pub async fn set(&self, value: Value) {
        loop {
            {
                let mut g = self.inner.lock().unwrap();
                if g.owner.is_none() {
                    g.value = value;
                    return;
                }
            }
            self.notify.notified().await;
        }
    }
}
