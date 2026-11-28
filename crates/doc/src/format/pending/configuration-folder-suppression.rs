//! Configuration table timeline-folder suppression (`Column::SuppressFolder(u64)`) saves a
//! new column variant whose value is `"true"` or `"false"`, suppressing or unsuppressing every
//! feature and joint in that folder and its nested folders. Older designs have no such column.
//! The last release warns about the newer format number, opens the design, and keeps the
//! unreadable configuration column intact when saving. No step: nothing older is rewritten.
