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
