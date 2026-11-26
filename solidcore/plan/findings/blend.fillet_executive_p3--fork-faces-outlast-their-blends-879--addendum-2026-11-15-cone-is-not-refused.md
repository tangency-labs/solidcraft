# blend.fillet_executive_p3: a cone with its apex on the face is not refused (#17198, 2026-11-15)

Updates: plan/findings/blend.fillet_executive_p3--fork-faces-outlast-their-blends-879.md

- **Not the fork's:** a plane face bounded by one closed edge whose blend's rail on it is a point (a cylinder's top chamfered by its radius) is a cone with its apex on the face, and is not refused. The refusal in the record above applies only to a blend that leaves a body with no room.
- **Test:** the test the record names as `chamfer_as_wide_as_its_face_is_refused` is now `chamfer_as_wide_as_its_face_is_a_cone`, in `crates/solidcore/tests/fillet_wr300_320.rs`.
