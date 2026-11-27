//! Drawings save their revisions (`revisions`, #3874): one revision record for the whole drawing,
//! its rows (revision, description, designer, checker, date, hidden) in the order they were added,
//! each with its id. An older drawing has no revisions. The last release refuses a drawing saved
//! with them, as a newer format.
