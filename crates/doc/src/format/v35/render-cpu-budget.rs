//! Saved CPU rendering intent can select Compact, Studio or Workstation resource
//! envelopes. Absent budget retains Compact exactly; larger envelopes never
//! imply a quality downgrade or runtime host availability. Existing designs need
//! no numeric rewrite. Older releases must refuse rather than discard the intent.
