//! A Thread feature can save its tolerance class as `thread_class` (ISO 965-1 for a metric thread,
//! ASME B1.1 for a Unified one), and the thread records a design keeps (the saved model's
//! `threads`, and a linked Import's `link_threads`) can carry `thread_class` and `modeled`. A
//! tapped hole already saved its class; its record now carries it too.
//! Older designs need no transformation: a Thread without the field has no class of its own (a
//! class written in its designation, `M8-6g`, still reads as written), and a record without
//! `modeled` is a cosmetic one, as every record was.
//! The last release opens such a design with a warning, ignores `thread_class`, reads a linked
//! modelled record as cosmetic (so it may draw cosmetic lines over the modelled thread), and keeps
//! what it can't read when it saves.
