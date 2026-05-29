//! Lint passes. Each pass lives in its own file; `all()` returns the default
//! ordered list consumed by `LintRunner::default_passes()`.
//!
//! Adding a pass: create a new file, declare its `pub mod` below, re-export
//! the marker struct, and append it to `all()`.

mod helpers;
mod scoped_visitor;

mod await_async_only;
mod contract_await_in_sync;
mod contract_wait_in_async;
mod define_external_access;
mod exclusive_self_primitive_await;
mod expose_mutable_unsafe;
mod expose_readonly_write;
mod generic_type_arg_missing;
mod handler_await_type;
mod handler_must_async;
mod handler_ping_redefined;
mod handler_result_ignored;
mod hook_signature;
mod permit_await_in_sync;
mod permit_release_non_positive;
mod permit_wait_in_async;
mod return_not_all_paths;
mod signal_await_in_sync;
mod signal_wait_in_async;
mod terminate_non_terminatable;
mod toplevel_control_flow;

pub use await_async_only::AwaitAsyncOnly;
pub use contract_await_in_sync::ContractAwaitInSync;
pub use contract_wait_in_async::ContractWaitInAsync;
pub use define_external_access::DefineExternalAccess;
pub use exclusive_self_primitive_await::ExclusiveSelfPrimitiveAwait;
pub use expose_mutable_unsafe::ExposeMutableUnsafe;
pub use expose_readonly_write::ExposeReadonlyWrite;
pub use generic_type_arg_missing::GenericTypeArgMissing;
pub use handler_await_type::HandlerAwaitType;
pub use handler_must_async::HandlerMustAsync;
pub use handler_ping_redefined::HandlerPingRedefined;
pub use handler_result_ignored::HandlerResultIgnored;
pub use hook_signature::HookSignature;
pub use permit_await_in_sync::PermitAwaitInSync;
pub use permit_release_non_positive::PermitReleaseNonPositive;
pub use permit_wait_in_async::PermitWaitInAsync;
pub use return_not_all_paths::ReturnNotAllPaths;
pub use signal_await_in_sync::SignalAwaitInSync;
pub use signal_wait_in_async::SignalWaitInAsync;
pub use terminate_non_terminatable::TerminateNonTerminatable;
pub use toplevel_control_flow::ToplevelControlFlow;

use crate::LintPass;

/// The default ordered list of lint passes used by `LintRunner`.
pub(crate) fn all() -> Vec<Box<dyn LintPass>> {
    vec![
        Box::new(AwaitAsyncOnly),
        Box::new(HandlerMustAsync),
        Box::new(HandlerAwaitType),
        Box::new(ExposeMutableUnsafe),
        Box::new(ExposeReadonlyWrite),
        Box::new(DefineExternalAccess),
        Box::new(ExclusiveSelfPrimitiveAwait),
        Box::new(GenericTypeArgMissing),
        Box::new(TerminateNonTerminatable),
        Box::new(PermitAwaitInSync),
        Box::new(PermitWaitInAsync),
        Box::new(PermitReleaseNonPositive),
        Box::new(SignalAwaitInSync),
        Box::new(SignalWaitInAsync),
        Box::new(ContractAwaitInSync),
        Box::new(ContractWaitInAsync),
        Box::new(ToplevelControlFlow),
        Box::new(HookSignature),
        Box::new(HandlerResultIgnored),
        Box::new(HandlerPingRedefined),
        Box::new(ReturnNotAllPaths),
    ]
}
