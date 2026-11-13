//! User materials can save a validated SI engineering card (`physical_card`) with property
//! conditions and source claims, independently of assigned mass density and visual appearance.
//! Older designs need no transformation: an absent card means unspecified engineering data.
//! The last release refuses this newer format; pending builds without this change refuse its
//! `material-engineering-cards` descriptor instead of silently dropping the saved card.
