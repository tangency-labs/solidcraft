//! Move body copies save each source's captured placement in the copy's component frame.
//! Older moves without these matrices keep copying the source's local geometry. The last
//! release warns about the newer format, opens what it can read, and keeps unknown fields.
