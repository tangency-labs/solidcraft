//! A Frame Member's corner override (`corners` entry) can hold `coped` (#4003): whether a butt at that
//! corner copes its butting tube to the through tube's real shape, whatever the frame's own `coped`
//! says. Older designs need no transformation: an override without it follows the frame's `coped`,
//! as before. The last release warns about the newer format and builds such a corner as the frame's
//! `coped` says, keeping the field when it saves the design again.
