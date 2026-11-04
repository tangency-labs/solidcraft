// SPDX-License-Identifier: Apache-2.0
//! Format Design diagnostics.

use cadmpeg_core::CodecError;
use std::fmt;

/// Build a malformed-Design diagnostic.
pub(super) fn malformed_design(arguments: fmt::Arguments<'_>) -> CodecError {
    CodecError::Malformed(arguments.to_string())
}
