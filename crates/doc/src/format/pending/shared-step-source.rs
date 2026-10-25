//! STEP imports may store a version-one shared STEP source rather than repeated geometry text.
//! Original entity and face identities remain explicit. Older designs keep their text source
//! unchanged; the previous release refuses designs bearing the newer format number.
//! Import filters may select planned body indices (`{"only":[...]}`); legacy arrays still skip
//! converted body indices. Components may retain their import feature id for compact browser
//! grouping, and STEP nodes retain product identity and placement names.
