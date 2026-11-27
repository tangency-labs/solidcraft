//! A modelled internal Thread can run down to a blind hole's bottom: its `to_bottom` is saved,
//! and the groove's last turn is then trimmed at the plane of the bottom instead of stopping clear
//! of it (#22616). Older designs have no `to_bottom`, so their threads stop clear of the bottom as
//! before. The last release warns that the design is newer, opens it, and builds such a thread
//! stopping clear of the bottom.
