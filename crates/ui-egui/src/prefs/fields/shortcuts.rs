//! Stored preferences on the Shortcuts page (`PAGE_SHORTCUTS`).
//!
//! Each entry is one field of `Prefs`, in `preference_settings!`'s form:
//! `field: Type = default, PAGE[, accept(range)][ => control(...)];`. Add a new preference
//! to the file of the page whose Defaults resets it, in alphabetical order by field name, so
//! two new preferences rarely land on the same line.

macro_rules! shortcuts_preferences {
    ($next:ident $(, $rest:ident)* ; $($fields:tt)*) => {
        $next! { $($rest),* ; $($fields)*
            /// While a sketch is open, only sketch keys and keys that work everywhere run; a model
            /// command's key (E, F) does nothing. Off (the default): a model key pressed in a sketch
            /// runs its command, and one that builds a feature finishes the sketch first (#2815).
            sketch_keys_only: bool = false, PAGE_SHORTCUTS;
        }
    };
}
