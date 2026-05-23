use thiserror::Error;
use tessera_ast::Span;

#[derive(Debug, Clone, Error)]
pub enum RuntimeError {
    #[error("panic: {message}")]
    Panic { message: String, location: Span },

    #[error("assertion failed: {message}")]
    AssertionFailed { message: String, location: Span },

    #[error("index out of bounds: index {index}, length {length}")]
    IndexOutOfBounds { index: i32, length: i32, location: Span },

    #[error("division by zero")]
    DivisionByZero { location: Span },

    #[error("unwrap on None")]
    UnwrapNone { location: Span },

    #[error("unwrap on Err")]
    UnwrapErr { location: Span },

    #[error("type mismatch: expected {expected}, got {got}")]
    TypeMismatch { expected: String, got: String, location: Span },

    #[error("undefined variable: {name}")]
    UndefinedVariable { name: String, location: Span },

    #[error("reentrant lock")]
    ReentrantLock { location: Span },

    #[error("unlock not owned")]
    UnlockNotOwned { location: Span },

    #[error("cannot replace expose_mutable field from outside")]
    ExposeMutableFieldReplace { location: Span },

    #[error("handler dispatch error: {0}")]
    HandlerDispatch(HandlerDispatchError),

    /// Structured error produced by async-context failures (broken sync primitives,
    /// handler dispatch/execution failures). `kind` is a stable identifier string;
    /// `message` is the human-readable description.
    #[error("[{kind}] {message}")]
    Structured { kind: String, message: String, location: Span },
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
