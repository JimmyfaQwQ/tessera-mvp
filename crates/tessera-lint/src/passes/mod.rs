//! Lint passes. Each pass lives in its own file; `all()` returns the default
//! ordered list consumed by `LintRunner::default_passes()`.
//!
//! Adding a pass: create a new file, declare its `pub mod` below, re-export
//! the marker struct, and append it to `all()`.

mod helpers;

mod await_async_only;
mod expose_mutable_unsafe;
mod generic_type_arg_missing;
mod handler_await_type;
mod handler_must_async;
mod permit_await_in_sync;
mod permit_release_non_positive;
mod permit_wait_in_async;
mod terminate_non_terminatable;

pub use await_async_only::AwaitAsyncOnly;
pub use expose_mutable_unsafe::ExposeMutableUnsafe;
pub use generic_type_arg_missing::GenericTypeArgMissing;
pub use handler_await_type::HandlerAwaitType;
pub use handler_must_async::HandlerMustAsync;
pub use permit_await_in_sync::PermitAwaitInSync;
pub use permit_release_non_positive::PermitReleaseNonPositive;
pub use permit_wait_in_async::PermitWaitInAsync;
pub use terminate_non_terminatable::TerminateNonTerminatable;

use crate::LintPass;

/// The default ordered list of lint passes used by `LintRunner`.
pub(crate) fn all() -> Vec<Box<dyn LintPass>> {
    vec![
        Box::new(AwaitAsyncOnly),
        Box::new(HandlerMustAsync),
        Box::new(HandlerAwaitType),
        Box::new(ExposeMutableUnsafe),
        Box::new(GenericTypeArgMissing),
        Box::new(TerminateNonTerminatable),
        Box::new(PermitAwaitInSync),
        Box::new(PermitWaitInAsync),
        Box::new(PermitReleaseNonPositive),
    ]
}
