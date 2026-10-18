//! The JNI calls we make in this crate are often not part of a Java native
//! method implementation and so we can't assume we have a JNI local frame that
//! is going to unwind and free local references, and we also can't just leave
//! exceptions to get thrown when returning to Java.
//!
//! These utilities help us check + clear exceptions and map them into Rust Errors.

use crate::error::InternalAppError;

/// Use with `.map_err()` to map `jni::errors::Error::JavaException` into a
/// richer error based on the actual contents of the `JThrowable`
///
/// (The `jni` crate doesn't do that automatically since it's more
/// common to let the exception get thrown when returning to Java)
///
/// This will also clear the exception
pub(crate) fn clear_and_map_exception_to_err(
    env: &mut jni::Env<'_>,
    err: jni::errors::Error,
) -> InternalAppError {
    if matches!(err, jni::errors::Error::JavaException) {
        env.exception_catch()
            .expect_err("Spurious JavaException error with no exception to catch")
    } else {
        err
    }
    .into()
}
