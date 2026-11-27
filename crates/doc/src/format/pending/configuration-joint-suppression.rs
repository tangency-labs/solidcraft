//! Configuration table joint suppression columns (#17458): `configs.columns` may include
//! `{"suppress_joint": <joint id>}` (`Column::SuppressJoint(u64)`) to store per-configuration
//! suppression for individual joints. Older designs have no `suppress_joint` columns and behave
//! identically. The last release warns about the newer format number, opens the design, and keeps
//! the unreadable `configs` section intact when saving. No migration step is needed.
