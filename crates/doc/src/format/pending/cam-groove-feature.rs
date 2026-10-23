//! A design can hold a Cam Groove feature: a groove cut along a path that a sketch draws on a
//! plane tangent to a cylindrical wall, and that is wrapped onto the wall before it is cut.
//! Older designs need no transformation: they have no such feature. The last release refuses a
//! design saved with this (the newer format number).

#[cfg(test)]
mod tests {
    /// The change needs no step: an older design has no Cam Groove, so there is nothing to
    /// rewrite. The build lists the change by name, and a design saved with it says so.
    #[test]
    fn the_change_needs_no_step_and_is_listed_by_name() {
        assert!(crate::format::pending_names().iter().any(|name| name == "cam-groove-feature"), "the change is listed");
        let mut document = crate::Document::new("older design");
        document
            .add_feature(
                crate::FeatureKind::Box {
                    corner: ["0".into(), "0".into(), "0".into()],
                    length: "10".into(),
                    width: "20".into(),
                    height: "5".into(),
                    operation: crate::Operation::NewBody,
                },
                None,
            )
            .unwrap();
        // Nothing rewrites a design that has no cam groove: it opens as it was saved.
        let opened = crate::format::read(&document.to_json()).unwrap();
        assert_eq!(opened, document);
    }
}
