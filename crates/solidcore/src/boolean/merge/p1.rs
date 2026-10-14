//! Boolean control flags of a merge (`SmMergeOptions`).
//!
//! Upstream: `source/SMLib/src/SmMerge.cpp`, `source/SMLib/inc/SmMerge.h`.
//!
//! | Rust                         | Upstream                                         |
//! |------------------------------|--------------------------------------------------|
//! | [`MergeOptions`]             | `SmMergeOptions` (fields, `Get*`, `SetGet*`)     |
//! | [`MergeOptions::new`]        | `SmMergeOptions::SmMergeOptions(...)`            |
//! | [`MergeOptions::default`]    | `SmMergeOptions()` (all default arguments)       |
//! | `Clone`                      | `SmMergeOptions(const SmMergeOptions&)`          |
//!
//! Deviations: CO-PORTED by unit `boolean.api_brep` (only the options record, which
//! `SmApiBooleanWithOptions` builds); the rest of unit `boolean.merge_p1` is not ported.
//! The getters, setters and `GetCanonical` / `SetCanonical` are the public fields.

/// Optional boolean flags of a merge (`SmMergeOptions`). The defaults are upstream's
/// (and the `SmMerge` constructor's): only `boolean_post_process` is set.
#[allow(clippy::struct_excessive_bools)] // upstream's five SmBoolean members
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MergeOptions {
    /// `m_bCookieCutter`: skip adding faces of the second B-rep to the first (only
    /// remove faces).
    pub cookie_cutter: bool,
    /// `m_bImprinting`: imprint (A intersect B) on the first B-rep and quit (as
    /// `SM_BO_IMPRINT`).
    pub imprinting: bool,
    /// `m_bBooleanPostProcess`: remove topological edges and vertices after the
    /// boolean.
    pub boolean_post_process: bool,
    /// `m_bImprintAndClassifyFaces`: imprint (A intersect B) on the first B-rep, mark
    /// the faces that would be deleted with `SM_AI_BOOLEAN_DELETED` and quit.
    pub imprint_and_classify_faces: bool,
    /// `m_bKeepOtherBrep`: do not delete the second B-rep (nor the first with
    /// `SM_BO_EXTRACT_SEPARATE`).
    pub keep_other_brep: bool,
}

impl MergeOptions {
    /// The options with every flag given (`SmMergeOptions::SmMergeOptions`).
    #[must_use]
    #[allow(clippy::fn_params_excessive_bools)] // upstream's constructor arguments
    pub const fn new(
        cookie_cutter: bool,
        imprinting: bool,
        boolean_post_process: bool,
        imprint_and_classify_faces: bool,
        keep_other_brep: bool,
    ) -> Self {
        Self {
            cookie_cutter,
            imprinting,
            boolean_post_process,
            imprint_and_classify_faces,
            keep_other_brep,
        }
    }
}

impl Default for MergeOptions {
    /// `SmMergeOptions()`: `(FALSE, FALSE, TRUE, FALSE, FALSE)`.
    fn default() -> Self {
        Self::new(false, false, true, false, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_upstream() {
        let o = MergeOptions::default();
        assert!(!o.cookie_cutter);
        assert!(!o.imprinting);
        assert!(o.boolean_post_process);
        assert!(!o.imprint_and_classify_faces);
        assert!(!o.keep_other_brep);
        assert_eq!(
            MergeOptions::new(true, false, false, true, true),
            MergeOptions {
                cookie_cutter: true,
                imprinting: false,
                boolean_post_process: false,
                imprint_and_classify_faces: true,
                keep_other_brep: true,
            }
        );
    }
}
