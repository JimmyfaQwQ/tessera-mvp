use miette::Diagnostic;
use thiserror::Error;
use tessera_ast::Span;

#[derive(Debug, Clone, Error, Diagnostic)]
pub enum RuntimeError {
    #[error("panic: {message}")]
    #[diagnostic(code(tessera::runtime::panic))]
    Panic {
        message: String,
        #[label("panicked here")]
        location: Span,
    },

    #[error("assertion failed: {message}")]
    #[diagnostic(
        code(tessera::runtime::assertion),
        help("the asserted condition evaluated to false at runtime")
    )]
    AssertionFailed {
        message: String,
        #[label("this assertion failed")]
        location: Span,
    },

    #[error("index out of bounds: index {index}, length {length}")]
    #[diagnostic(
        code(tessera::runtime::index_out_of_bounds),
        help("valid indices are 0..length; check the length before indexing")
    )]
    IndexOutOfBounds {
        index: i32,
        length: i32,
        #[label("index {index} is out of range for length {length}")]
        location: Span,
    },

    #[error("division by zero")]
    #[diagnostic(
        code(tessera::runtime::division_by_zero),
        help("ensure the divisor is non-zero before dividing")
    )]
    DivisionByZero {
        #[label("this expression divides by zero")]
        location: Span,
    },

    #[error("unwrap on None")]
    #[diagnostic(
        code(tessera::runtime::unwrap_none),
        help("guard with `isSome()` or use `unwrapOr(default)` before unwrapping an Option")
    )]
    UnwrapNone {
        #[label("unwrapped a `None` value")]
        location: Span,
    },

    #[error("unwrap on Err")]
    #[diagnostic(
        code(tessera::runtime::unwrap_err),
        help("guard with `isOk()` or use `unwrapOr(default)` before unwrapping a Result")
    )]
    UnwrapErr {
        #[label("unwrapped an `Err` value")]
        location: Span,
    },

    #[error("type mismatch: expected {expected}, got {got}")]
    #[diagnostic(code(tessera::runtime::type_mismatch))]
    TypeMismatch {
        expected: String,
        got: String,
        #[label("expected {expected}, got {got}")]
        location: Span,
    },

    #[error("undefined variable: {name}")]
    #[diagnostic(
        code(tessera::runtime::undefined_variable),
        help("declare it with `let` before use, or check for a typo")
    )]
    UndefinedVariable {
        name: String,
        #[label("`{name}` is not defined in this scope")]
        location: Span,
    },

    #[error("reentrant lock")]
    #[diagnostic(
        code(tessera::runtime::reentrant_lock),
        help("this lock is already held by the current thread")
    )]
    ReentrantLock {
        #[label("re-locked here")]
        location: Span,
    },

    #[error("unlock not owned")]
    #[diagnostic(
        code(tessera::runtime::unlock_not_owned),
        help("only the thread that acquired the lock may release it")
    )]
    UnlockNotOwned {
        #[label("unlocked here")]
        location: Span,
    },

    #[error("cannot replace expose_mutable field from outside")]
    #[diagnostic(
        code(tessera::runtime::expose_mutable_replace),
        help("mutate the field via its own methods instead of reassigning it")
    )]
    ExposeMutableFieldReplace {
        #[label("this assignment is not allowed")]
        location: Span,
    },

    #[error("handler dispatch error: {0}")]
    #[diagnostic(code(tessera::runtime::handler_dispatch))]
    HandlerDispatch(HandlerDispatchError),

    /// Structured error produced by async-context failures (broken sync primitives,
    /// handler dispatch/execution failures). `kind` is a stable identifier string;
    /// `message` is the human-readable description.
    #[error("[{kind}] {message}")]
    #[diagnostic(code(tessera::runtime::structured))]
    Structured {
        kind: String,
        message: String,
        #[label("raised here")]
        location: Span,
    },
}

impl RuntimeError {
    pub fn location(&self) -> Span {
        match self {
            RuntimeError::Panic { location, .. }
            | RuntimeError::AssertionFailed { location, .. }
            | RuntimeError::IndexOutOfBounds { location, .. }
            | RuntimeError::DivisionByZero { location }
            | RuntimeError::UnwrapNone { location }
            | RuntimeError::UnwrapErr { location }
            | RuntimeError::TypeMismatch { location, .. }
            | RuntimeError::UndefinedVariable { location, .. }
            | RuntimeError::ReentrantLock { location }
            | RuntimeError::UnlockNotOwned { location }
            | RuntimeError::ExposeMutableFieldReplace { location }
            | RuntimeError::Structured { location, .. } => *location,
            RuntimeError::HandlerDispatch(_) => Span::dummy(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum HandlerDispatchError {
    #[error("target thread has terminated")]
    TargetTerminated,
    #[error("target thread is terminating")]
    TargetTerminating,
    #[error("target thread has crashed")]
    TargetCrashed,
}
