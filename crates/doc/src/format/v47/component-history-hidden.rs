//! A component marked out of the History bar saves `history_hidden: true` on its component
//! entry, so its features (and its children's) stay out of the History bar while still
//! rebuilding. Older designs carry no such flag and every component shows its features, which
//! is exactly the flag's default; the last release refuses this newer format and asks for an
//! update instead of silently dropping the flag on the next save.
