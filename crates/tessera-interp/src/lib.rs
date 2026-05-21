mod env;
mod eval;
mod event_loop;

pub use eval::Interpreter;

use tessera_ast::Program;
use tessera_runtime::RuntimeError;

pub async fn run(program: &Program) -> Result<(), RuntimeError> {
    let local = tokio::task::LocalSet::new();
    local.run_until(async {
        let interp = Interpreter::new();
        interp.run_program(program).await
    }).await
}
