//! Tessera interpreter.
//!
//! # Single-threaded execution model
//!
//! The interpreter is intentionally **not `Send`**. All evaluation runs inside a
//! single `tokio::task::LocalSet` on one OS thread (see [`run_reported`]). This
//! lets `InterpState` use cheap `Rc<RefCell<…>>` for shared mutable state across
//! Tessera threads and mini-threads — cooperative scheduling within the
//! `LocalSet` guarantees no concurrent borrows.
//!
//! Consequences:
//!
//! - Tessera concurrency is cooperative, not parallel. Two Tessera threads will
//!   not run on two OS threads.
//! - `Arc<Mutex<…>>` appears in some runtime primitives so that the trait
//!   `BreakablePrimitive: Send + Sync` is satisfiable, even though the values
//!   never actually cross OS-thread boundaries at runtime. The clippy lint
//!   `arc_with_non_send_sync` is silenced at the construction sites.
//! - Embedders must not try to share an [`Interpreter`] across OS threads.

// Justified by the single-threaded invariant documented above: the lint flags
// `Arc<Mutex<NotSendSync>>` constructions in `tessera_runtime` that the runtime
// only ever shares within one `LocalSet`.
#![allow(clippy::arc_with_non_send_sync)]

mod env;
mod eval;
mod event_loop;

pub use eval::{Frame, Interpreter};

use tessera_ast::Program;
use tessera_runtime::RuntimeError;

/// A runtime failure together with the call-stack traceback captured at the
/// deepest point the error was observed.
#[derive(Debug, Clone)]
pub struct RuntimeReport {
    pub error: RuntimeError,
    pub backtrace: Vec<Frame>,
}

/// Run a program, returning a structured report (error + traceback) on failure.
pub async fn run_reported(program: &Program) -> Result<(), RuntimeReport> {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let interp = Interpreter::new();
            match interp.run_program(program).await {
                Ok(()) => Ok(()),
                Err(error) => {
                    let backtrace = interp.0.last_backtrace.borrow().clone().unwrap_or_default();
                    Err(RuntimeReport { error, backtrace })
                }
            }
        })
        .await
}

/// Run a program, returning just the runtime error on failure.
pub async fn run(program: &Program) -> Result<(), RuntimeError> {
    run_reported(program).await.map_err(|r| r.error)
}
