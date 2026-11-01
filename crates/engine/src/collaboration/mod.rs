//! Live co-editing (#5184): who may do what in a shared design.
//!
//! A collaborator's set of permissions is a [`Role`]: a name and a list of rules. Creator,
//! Worker and Viewer are the built-in roles, made of rules; custom roles come later on the same
//! rules. A role is compiled once into an [`Access`], which answers every question with one
//! function, [`Access::allows`]. Who has which role comes from the repository's member list
//! (`collaboration/members.json`), read by [`MemberList::read`].

mod role;

pub use role::{Ability, Access, Ask, Effect, Member, MemberKind, MemberList, Pattern, Reason, Receive, Role, Rule, TargetKind, TargetRule, What};
