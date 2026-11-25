//! Linked Import features save cosmetic thread records in `link_threads`, indexed by the
//! imported file's bodies. Older designs start with no records; Update Links reads them from
//! the source. The last release warns about the newer format, opens what it can read and
//! preserves the new field when saving.
