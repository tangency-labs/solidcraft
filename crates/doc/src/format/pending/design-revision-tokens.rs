//! Saves the document identity and opaque committed history-state revision for plugin results.
//! A revision combines a 128-bit process epoch with the existing history generation, so
//! reopening in another process cannot repeat a different process's generation token.
//! Older designs receive a fresh identity and revision on first read; saving records them
//! for subsequent opens. The last release warns about the newer format, opens what it
//! understands and retains these unknown fields when saving (`crate::newer_file`).
