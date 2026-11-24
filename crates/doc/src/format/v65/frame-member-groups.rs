//! A `frame_member` feature can hold `groups` (#4003): two or more groups of its frame sketch's
//! lines, each with its own `lines`, `anchor`, `locate`, `angle`, `mirror` and `body_name`, all
//! made of the feature's one profile. Then the feature's own `lines`, `anchor`, `locate`, `angle`,
//! `mirror` and `body_name` stay unset. Older designs need no transformation: without `groups` those fields
//! are the feature's one group, as before. An older release warns about the newer format and
//! preserves the group settings it cannot read when saving the design again.
