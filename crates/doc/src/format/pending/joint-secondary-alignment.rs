//! Face and circle joint origins may keep a strict planar secondary x reference, named
//! `planar_face`, with an independent `reverse` flag. Existing x references and joint angles
//! keep their meaning; no alignment is added to older designs.
//! A typed `reference` also identifies a straight/circular named edge, cylindrical face,
//! construction feature, sketch line, or the owning component's origin axis/plane. Its
//! identity and frame are resolved again; missing or ambiguous geometry has no fallback.
