//! Body patterns may save an optional `body_name_template` (the command’s `body_names`) template, applied to their new bodies on
//! every rebuild. The original keeps its name; `{i}` numbers new bodies from 1 in copy order.
//! Older designs omit the field and keep the existing copy names without migration.
//! The last release opens a design with this newer format with a warning and keeps fields it
//! cannot read on save, through `crate::newer_file`.
