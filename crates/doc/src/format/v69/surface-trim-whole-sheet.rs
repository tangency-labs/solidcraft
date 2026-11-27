//! Surface Trim saves `tool_body` for a complete bounded cutting sheet, independently of
//! `tool`, which still names one extended face. Older designs omit `tool_body` and retain
//! their face, plane or curve tool. The last release warns about the newer format, opens
//! what it can read and preserves fields it cannot read when saving.
