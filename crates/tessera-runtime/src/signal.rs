use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

// ── Broken state ─────────────────────────────────────────────────────────────

/// Why a synchronization primitive entered Broken state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BrokenReason {
    /// Binding owner completed normal termination (Terminated).
    OwnerGone,
    /// Binding owner crashed (Crashed).
    OwnerCrashed,
    /// Scope binding: @template block exited normally (__on_exit__ returned).
    ScopeGone,
    /// Scope binding: enclosing thread crashed while the @template scope was active.
    ScopeCrashed,
}

impl BrokenReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            BrokenReason::OwnerGone    => "OwnerGone",
            BrokenReason::OwnerCrashed => "OwnerCrashed",
            BrokenReason::ScopeGone    => "ScopeGone",
            BrokenReason::ScopeCrashed => "ScopeCrashed",
        }
    }
}

/// Trait implemented by all three sync primitives so `ThreadState` can break
/// them without knowing their concrete types.
pub trait BreakablePrimitive: Send + Sync {
    fn break_with(&self, reason: BrokenReason);
}

// ── Shared helpers (signal & contract) ────────────────────────────────────────

/// First-expose-wins ownership flag used by all three primitives.
///
/// Once `try_claim()` returns true, every subsequent call returns false.
/// The interpreter uses this so a primitive is bound to at most one thread.
#[derive(Debug)]
struct Ownership(AtomicBool);

impl Ownership {
    const fn new() -> Self { Self(AtomicBool::new(false)) }

    fn try_claim(&self) -> bool {
        self.0.compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed).is_ok()
    }
}

/// Read-only accessors shared by `TesseraSignal` and `TesseraContract`, both of
/// which hold a `Mutex<S>` where `S` has a `broken: Option<BrokenReason>` field.
/// `TesseraPermit` uses `tokio::Semaphore::close()` for broken state and so does
/// not participate.
trait HasBroken {
    fn read_broken(&self) -> Option<BrokenReason>;
}

#[inline]
fn broken_is_some<S: HasBroken>(state: &S) -> bool { state.read_broken().is_some() }

#[inline]
fn broken_is_none<S: HasBroken>(state: &S) -> bool { state.read_broken().is_none() }

impl HasBroken for SignalState {
    fn read_broken(&self) -> Option<BrokenReason> { self.broken.clone() }
}

impl HasBroken for ContractState {
    fn read_broken(&self) -> Option<BrokenReason> { self.broken.clone() }
}

// ── signal ────────────────────────────────────────────────────────────────────

struct SignalState {
    raised: bool,
    broken: Option<BrokenReason>,
}

/// Broadcast signal — manual-reset event.
///
/// - `raise()` sets the signal; wakes ALL current and future waiters.
/// - `reset()` clears the signal; subsequent waiters will block again.
/// - When the binding owner terminates/crashes, the signal enters `Broken` state:
///   all current waiters are woken with failure; future waits fail immediately.
pub struct TesseraSignal {
    state: Mutex<SignalState>,
    /// Notified on every state change (raise, reset, broken).
    changed: tokio::sync::Notify,
    /// True once ownership has been claimed (first expose wins).
    claimed: Ownership,
}

impl std::fmt::Debug for TesseraSignal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = self.state.lock().unwrap();
        if s.broken.is_some() {
            write!(f, "signal(broken)")
        } else {
            write!(f, "signal({})", if s.raised { "raised" } else { "not raised" })
        }
    }
}

impl Default for TesseraSignal {
    fn default() -> Self { Self::new() }
}

impl TesseraSignal {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(SignalState { raised: false, broken: None }),
            changed: tokio::sync::Notify::new(),
            claimed: Ownership::new(),
        }
    }

    /// Atomically set to raised; wakes all waiters. No-op if broken.
    pub fn raise(&self) {
        let mut s = self.state.lock().unwrap();
        if s.broken.is_some() { return; }
        s.raised = true;
        drop(s);
        self.changed.notify_waiters();
    }

    /// Atomically set to not-raised. No-op if broken.
    pub fn reset(&self) {
        let mut s = self.state.lock().unwrap();
        if s.broken.is_some() { return; }
        s.raised = false;
        // No wakeup needed — waiters re-check on the next raise.
    }

    /// Snapshot: true only if raised AND not broken.
    pub fn is_raised(&self) -> bool {
        let s = self.state.lock().unwrap();
        s.raised && s.broken.is_none()
    }

    pub fn is_ok(&self)  -> bool { broken_is_none(&*self.state.lock().unwrap()) }
    pub fn is_err(&self) -> bool { broken_is_some(&*self.state.lock().unwrap()) }

    pub fn broken_reason(&self) -> Option<BrokenReason> {
        self.state.lock().unwrap().read_broken()
    }

    /// First expose wins ownership. Returns true if this call claimed it.
    pub fn try_claim_ownership(&self) -> bool { self.claimed.try_claim() }

    /// Suspend until raised. Returns `Err` if the signal is (or becomes) broken.
    /// The caller is responsible for converting `Err` into a thread panic.
    pub async fn wait(&self) -> Result<(), BrokenReason> {
        loop {
            // Pin a waiter entry BEFORE checking state (race-free).
            let fut = self.changed.notified();
            {
                let s = self.state.lock().unwrap();
                if s.raised { return Ok(()); }
                if let Some(r) = &s.broken { return Err(r.clone()); }
            }
            fut.await;
        }
    }
}

impl BreakablePrimitive for TesseraSignal {
    fn break_with(&self, reason: BrokenReason) {
        let mut s = self.state.lock().unwrap();
        if s.broken.is_some() { return; }
        s.broken = Some(reason);
        drop(s);
        self.changed.notify_waiters(); // wake ALL current waiters
    }
}

// ── contract ──────────────────────────────────────────────────────────────────

struct ContractState {
    pending: bool,
    broken: Option<BrokenReason>,
}

/// Auto-reset single-cast event with FIFO wait queue.
///
/// - `fulfill()` delivers one notification; consumed by exactly one waiter.
/// - If no waiter is parked, stores the notification for the next `wait()`.
/// - Idempotent: calling `fulfill()` when already pending is a no-op.
pub struct TesseraContract {
    state: Mutex<ContractState>,
    changed: tokio::sync::Notify,
    claimed: Ownership,
}

impl std::fmt::Debug for TesseraContract {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = self.state.lock().unwrap();
        if s.broken.is_some() {
            write!(f, "contract(broken)")
        } else {
            write!(f, "contract({})", if s.pending { "pending" } else { "idle" })
        }
    }
}

impl Default for TesseraContract {
    fn default() -> Self { Self::new() }
}

impl TesseraContract {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(ContractState { pending: false, broken: None }),
            changed: tokio::sync::Notify::new(),
            claimed: Ownership::new(),
        }
    }

    /// Deliver one notification. No-op if broken or already pending.
    pub fn fulfill(&self) {
        let mut s = self.state.lock().unwrap();
        if s.broken.is_some() || s.pending { return; }
        s.pending = true;
        drop(s);
        self.changed.notify_one(); // wake exactly one waiter (FIFO)
    }

    /// Snapshot: true only if pending AND not broken.
    pub fn is_pending(&self) -> bool {
        let s = self.state.lock().unwrap();
        s.pending && s.broken.is_none()
    }

    pub fn is_ok(&self)  -> bool { broken_is_none(&*self.state.lock().unwrap()) }
    pub fn is_err(&self) -> bool { broken_is_some(&*self.state.lock().unwrap()) }

    pub fn broken_reason(&self) -> Option<BrokenReason> {
        self.state.lock().unwrap().read_broken()
    }

    pub fn try_claim_ownership(&self) -> bool { self.claimed.try_claim() }

    /// Consume one notification; suspends if none is available.
    /// Returns `Err` if broken with no pending notification.
    ///
    /// Pending-before-broken: if `fulfill()` was called before the owner
    /// crashed, the notification is still deliverable. Check `pending` first
    /// so that `wait()` succeeds even when `broken` is also set.
    pub async fn wait(&self) -> Result<(), BrokenReason> {
        loop {
            // Pin waiter entry BEFORE checking state (race-free).
            let fut = self.changed.notified();
            {
                let mut s = self.state.lock().unwrap();
                // Consume a pending notification even if the primitive is
                // already broken: the notification was delivered before the
                // owner died and must not be silently discarded.
                if s.pending {
                    s.pending = false;
                    return Ok(());
                }
                if let Some(r) = &s.broken { return Err(r.clone()); }
            }
            fut.await;
        }
    }
}

impl BreakablePrimitive for TesseraContract {
    fn break_with(&self, reason: BrokenReason) {
        let mut s = self.state.lock().unwrap();
        if s.broken.is_some() { return; }
        s.broken = Some(reason);
        drop(s);
        self.changed.notify_waiters(); // wake ALL (unlike fulfill which wakes one)
    }
}

// ── permit ────────────────────────────────────────────────────────────────────

/// Counting semaphore with FIFO wait queue.
///
/// - `release()` / `release_n(n)` adds permits; wakes FIFO waiters.
/// - `wait()` / `await p` consumes one permit; suspends if count is zero.
/// - `Semaphore::close()` is used to implement Broken: wakes all waiters.
pub struct TesseraPermit {
    sem: tokio::sync::Semaphore,
    broken: Mutex<Option<BrokenReason>>,
    claimed: Ownership,
}

impl std::fmt::Debug for TesseraPermit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.sem.is_closed() {
            write!(f, "permit(broken)")
        } else {
            write!(f, "permit({})", self.sem.available_permits())
        }
    }
}

impl TesseraPermit {
    pub fn new(initial: i32) -> Self {
        assert!(initial >= 0, "permit: initial must be non-negative");
        Self {
            sem: tokio::sync::Semaphore::new(initial as usize),
            broken: Mutex::new(None),
            claimed: Ownership::new(),
        }
    }

    /// Return one permit; wake the oldest waiter if any. No-op if broken.
    pub fn release(&self) {
        if self.sem.is_closed() { return; }
        self.sem.add_permits(1);
    }

    /// Return `n` permits. `n` must be > 0. No-op if broken.
    pub fn release_n(&self, n: i32) {
        assert!(n > 0, "permit: release(n) requires n > 0");
        if self.sem.is_closed() { return; }
        self.sem.add_permits(n as usize);
    }

    /// Snapshot of available permits. Returns 0 if broken.
    pub fn count(&self) -> i32 {
        if self.sem.is_closed() { return 0; }
        self.sem.available_permits() as i32
    }

    pub fn is_ok(&self)  -> bool { !self.sem.is_closed() }
    pub fn is_err(&self) -> bool { self.sem.is_closed() }

    pub fn broken_reason(&self) -> Option<BrokenReason> {
        self.broken.lock().unwrap().clone()
    }

    pub fn try_claim_ownership(&self) -> bool { self.claimed.try_claim() }

    /// Acquire one permit; suspends (FIFO) until one is available.
    /// Returns `Err` if broken.
    pub async fn acquire(&self) -> Result<(), BrokenReason> {
        match self.sem.acquire().await {
            Ok(p) => { p.forget(); Ok(()) }
            Err(_) => {
                // Semaphore was closed (Broken).
                let reason = self.broken.lock().unwrap().clone()
                    .unwrap_or(BrokenReason::OwnerGone);
                Err(reason)
            }
        }
    }
}

impl BreakablePrimitive for TesseraPermit {
    fn break_with(&self, reason: BrokenReason) {
        let mut b = self.broken.lock().unwrap();
        if b.is_some() { return; }
        *b = Some(reason);
        drop(b);
        self.sem.close(); // wakes all waiters with AcquireError
    }
}
