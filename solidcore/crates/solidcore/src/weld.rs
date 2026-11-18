//! Welds: display geometry for welded joints, built as bodies of their own beside the parts
//! they join. A weld never changes a part; its bodies take no part as input. Why a weld is
//! refused is one enum for every weld type, [`crate::error::weld::WeldFailure`].
//!
//! Not upstream: SolidCraft's own (#15132).

pub mod groove;
