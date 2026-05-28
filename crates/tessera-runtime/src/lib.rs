//! Tessera runtime — values, threads, and sync primitives.
//!
//! # Mutex poisoning policy
//!
//! Sync primitives in this crate (`TesseraLocked`, `TesseraSignal`,
//! `TesseraContract`, `TesseraPermit`, `ThreadState`) use `std::sync::Mutex`
//! and call `.lock().unwrap()` throughout. The critical sections are
//! deliberately short and contain no panicking operations (no allocation of
//! unbounded size, no indexing, no user-supplied callbacks), so the mutex can
//! never be poisoned in practice. Treat `lock().unwrap()` as a documented
//! contract, not an oversight: if any future change introduces a panic inside
//! a critical section, switch that primitive to `parking_lot::Mutex`
//! (already a workspace dependency, doesn't poison) rather than try to recover.
//!
//! # Threading model
//!
//! See `tessera-interp` crate docs: the whole interpreter runs in a single
//! `LocalSet`, so the `Send + Sync` bounds on `BreakablePrimitive` exist only
//! to satisfy the trait — values never actually cross OS-thread boundaries.

// Justified by the threading-model note above: the lint flags
// `Arc<Mutex<NotSendSync>>` constructions for primitives whose contents
// (e.g. `Value` chains containing `Rc<RefCell<…>>`) cannot be `Send`, but they
// are only ever shared within one `LocalSet`.
#![allow(clippy::arc_with_non_send_sync)]

// Modules stay `pub(crate)`; only the explicit re-exports below form the
// public API. Adding a type that needs to be visible to `tessera-interp`
// requires a deliberate re-export, which is easier to audit than module-path
// imports across crates.
pub(crate) mod error;
pub(crate) mod value;
pub(crate) mod thread_state;
pub(crate) mod locked;
pub(crate) mod queue;
pub(crate) mod future;
pub(crate) mod signal;

pub use error::{RuntimeError, HandlerDispatchError};
pub use value::{Value, ValueKey};
pub use thread_state::{ThreadId, ThreadState, ThreadStatus, HandlerRequest, HandlerOutcome, TerminateBundle};
pub use locked::TesseraLocked;
pub use queue::{TesseraQueue, QueuePushError};
pub use future::{TesseraFuture, TesseraHandlerFuture, FutureOutcome, HandlerResolveResult};
pub use signal::{TesseraSignal, TesseraContract, TesseraPermit, BrokenReason, BreakablePrimitive};
