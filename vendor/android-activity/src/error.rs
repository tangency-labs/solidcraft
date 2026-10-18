use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Operation only supported from the android_main() thread: {0}")]
    NonMainThread(String),

    #[error("Java VM or JNI error, including Java exceptions")]
    JavaError(String),

    #[error("Input unavailable")]
    InputUnavailable,
}

pub type Result<T> = std::result::Result<T, AppError>;

// XXX: we don't want to expose jni-rs in the public API
// so we have an internal error type that we can generally
// use in the backends and then we can strip the error
// in the frontend of the API.
//
// This way we avoid exposing a public trait implementation for
// `From<jni::errors::Error>`
#[derive(Error, Debug)]
pub(crate) enum InternalAppError {
    #[error("A JNI error")]
    JniError(jni::errors::JniError),
    // For internal errors that don't lead to a Java exception but are
    // still JNI related.
    #[error("A bad argument was passed to a JNI method: {0}")]
    JniBadArgument(String),
    #[error("A Java VM error")]
    JvmError(jni::errors::Error),
    #[error("Input unavailable")]
    InputUnavailable,
}

pub(crate) type InternalResult<T> = std::result::Result<T, InternalAppError>;

impl From<jni::errors::Error> for InternalAppError {
    fn from(value: jni::errors::Error) -> Self {
        InternalAppError::JvmError(value)
    }
}
impl From<jni::errors::JniError> for InternalAppError {
    fn from(value: jni::errors::JniError) -> Self {
        InternalAppError::JniError(value)
    }
}

impl From<InternalAppError> for AppError {
    fn from(value: InternalAppError) -> Self {
        match value {
            InternalAppError::JniError(err) => AppError::JavaError(err.to_string()),
            InternalAppError::JniBadArgument(msg) => AppError::JavaError(msg),
            InternalAppError::JvmError(err) => AppError::JavaError(err.to_string()),
            InternalAppError::InputUnavailable => AppError::InputUnavailable,
        }
    }
}
