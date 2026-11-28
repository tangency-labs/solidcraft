//! An edge flange feature can move its bends out past where its `position` starts them:
//! `offset` is a length, and the panel the flange stands on grows by the strip between (a
//! negative offset moves the bends in). A flange with no offset, as every flange was before,
//! saves no `offset`, so older designs need no step and open and save as they were. The last
//! release warns about the newer format number, opens what it can read and keeps the rest.
