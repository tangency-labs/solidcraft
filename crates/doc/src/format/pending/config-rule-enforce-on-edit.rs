//! A configuration rule can carry `enforce_on_edit: true`: its range or allowed-values restriction
//! then also holds when a parameter is changed by an ordinary edit (`parameters.change`), and the
//! edit is refused naming the rule, not only when a row is activated. A generator's design sets it
//! on the limits it declares (a rail's length range). An older design has no flag and needs no
//! step: its rules keep today's behavior (enforced at activation and preview only). The last
//! release warns about the newer format number and names this change among those it doesn't know
//! (`crates/doc/src/newer_file.rs`); it does not read the flag, so it ignores the restriction on an
//! ordinary edit, and a save by it drops the flag.
