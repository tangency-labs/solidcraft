//! An issued revision saves the design it was issued from as `model_version: {path, change}`, opaque
//! text from the version history, on its issued copy only. It was a string that no command ever
//! wrote, so no step rewrites older drawings. A row's issued copy keeps the row's id, one copy for
//! each row. The last release refuses a drawing with this format's number.
