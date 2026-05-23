use std::sync::Mutex;
use tokio::sync::watch;

/// Broadcast signal — manual-reset event.
///
/// - `raise()` sets the signal; all current and future waiters are unblocked.
/// - `reset()` clears the signal; subsequent waiters will block again.
/// - Concurrent-safe: can be shared across Tessera threads via `expose_mutable`.
pub struct TesseraSignal {
    tx: watch::Sender<bool>,
    // Keep one receiver alive so that send() always has a subscriber and
    // actually stores the new value.  Without this, send() returns Err when
    // there are no external waiters and the stored value is never updated.
    _keep: watch::Receiver<bool>,
}

// watch::Sender is Send + Sync, so TesseraSignal is too.
impl std::fmt::Debug for TesseraSignal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "signal({})", if *self.tx.borrow() { "raised" } else { "not raised" })
    }
}

impl TesseraSignal {
    pub fn new() -> Self {
        let (tx, _keep) = watch::channel(false);
        Self { tx, _keep }
    }

    /// Atomically set signal to raised; wakes all waiters.
    pub fn raise(&self) {
        let _ = self.tx.send(true);
    }

    /// Atomically reset signal to not-raised.
    pub fn reset(&self) {
        let _ = self.tx.send(false);
    }

    /// Snapshot of current state.
    pub fn is_raised(&self) -> bool {
        *self.tx.borrow()
    }

    /// Suspend until the signal is raised. Returns immediately if already raised.
    pub async fn wait(&self) {
        let mut rx = self.tx.subscribe();
        // borrow_and_update marks the current value as "seen", so changed() will
        // only fire on the *next* send — we explicitly check the current value first.
        if *rx.borrow_and_update() {
            return;
        }
        loop {
            if rx.changed().await.is_err() {
                return; // sender dropped
            }
            if *rx.borrow_and_update() {
                return;
            }
        }
    }
}

/// `permit` — counting semaphore (FIFO).
///
/// - `release()` / `release_n(n)` adds permits; wakes queued waiters FIFO.
/// - `acquire()` consumes one permit; suspends until one is available.
/// - Concurrent-safe.
pub struct TesseraPermit {
    sem: tokio::sync::Semaphore,
}

impl std::fmt::Debug for TesseraPermit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "permit({})", self.sem.available_permits())
    }
}

impl TesseraPermit {
    pub fn new(initial: i32) -> Self {
        assert!(initial >= 0, "permit: initial must be non-negative");
        Self { sem: tokio::sync::Semaphore::new(initial as usize) }
    }

    /// Return one permit; wake the oldest waiter if any.
    pub fn release(&self) {
        self.sem.add_permits(1);
    }

    /// Return `n` permits. Panics if `n <= 0`.
    pub fn release_n(&self, n: i32) {
        assert!(n > 0, "permit: release(n) requires n > 0");
        self.sem.add_permits(n as usize);
    }

    /// Snapshot of current available permits.
    pub fn count(&self) -> i32 {
        self.sem.available_permits() as i32
    }

    /// Acquire one permit; suspends (FIFO) until one is available.
    pub async fn acquire(&self) {
        // forget() so the permit is not auto-released on drop;
        // the Tessera program calls release() explicitly.
        let p = self.sem.acquire().await.unwrap();
        p.forget();
    }
}

/// `contract` — auto-reset single-waiter (FIFO) event.
///
/// - `fulfill()` stores one notification; the next waiter consumes it.
/// - Each notification is consumed by exactly one waiter (FIFO).
/// - If called when a notification is already pending, it is a no-op.
/// - Concurrent-safe.
pub struct TesseraContract {
    pending: Mutex<bool>,
    notify: tokio::sync::Notify,
}

impl std::fmt::Debug for TesseraContract {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let p = self.pending.lock().map(|g| *g).unwrap_or(false);
        write!(f, "contract({})", if p { "pending" } else { "idle" })
    }
}

impl TesseraContract {
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(false),
            notify: tokio::sync::Notify::new(),
        }
    }

    /// Store one notification. Idempotent if a notification is already pending.
    pub fn fulfill(&self) {
        let mut p = self.pending.lock().unwrap();
        if !*p {
            *p = true;
            self.notify.notify_one();
        }
    }

    /// Snapshot: true if a notification is waiting to be consumed.
    pub fn is_pending(&self) -> bool {
        *self.pending.lock().unwrap()
    }

    /// Consume one notification; blocks/suspends if none is available.
    pub async fn wait(&self) {
        // Check for a stored notification before parking.
        {
            let mut p = self.pending.lock().unwrap();
            if *p {
                *p = false;
                return;
            }
        }
        self.notify.notified().await;
        // Consume the stored notification that fulfill() set before notify_one().
        let mut p = self.pending.lock().unwrap();
        *p = false;
    }
}
