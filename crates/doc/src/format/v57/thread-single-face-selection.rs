//! A Thread feature can save single_face_selection to keep single-face replacement in its
//! dialog while target_bodies retains the selected body's identity. Older designs omit this
//! flag and keep their previous selection mode. The last release opens the newer format with
//! a warning and preserves fields it cannot read.
