//! A body's display plan in the form the viewport evaluates on the GPU. The kernel plans with the
//! one mesher in solidcore-tess (RFC 0003, item 1.1), so that form is the plan itself; the model
//! still caches it apart from the display plan, so the viewport's cache, keyed by each plan's
//! allocation, keeps its uploads from frame to frame.

use solidcraft_geom::tess::TessPlan;

/// `plan` in the form the viewport evaluates on the GPU: solidcore-tess's plan, which the kernel's
/// already is.
pub fn gpu_plan(plan: &TessPlan) -> solidcore_tess::TessPlan {
    plan.clone()
}

#[cfg(test)]
mod tests {
    use solidcore_record::Vec3;

    /// A body's GPU plan is made once per display quality and then read as the same allocation,
    /// by the body and its clones (the viewport keys its uploads by it), equal to its display plan
    /// converted.
    #[test]
    fn a_bodys_gpu_plan_is_one_allocation_across_reads() {
        let body = solidcraft_kernel::box_solid(Vec3::ZERO, Vec3::new(10.0, 8.0, 4.0)).unwrap();
        let b = crate::ModelBody::new("Body1".into(), body, 0);
        let q = crate::display_quality::DisplayQuality::default();
        let first = b.gpu_plan_at(q).unwrap();
        assert!(std::sync::Arc::ptr_eq(&first, &b.gpu_plan_at(q).unwrap()));
        assert!(std::sync::Arc::ptr_eq(&first, &b.clone().gpu_plan_at(q).unwrap()));
        assert!(std::sync::Arc::ptr_eq(&first, &b.refined_display(q, 0).unwrap().0));
        assert_eq!(*first, super::gpu_plan(&b.display_plan_at(q).unwrap()));
    }
}
