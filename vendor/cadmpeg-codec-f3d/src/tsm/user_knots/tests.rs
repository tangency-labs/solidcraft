// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::{scalar::NonNegativeReal, subd::SubdUserKnot};

#[test]
fn user_knot_regeneration_preserves_deleted_slots_zero_and_extension_absence() {
    let bytes = crate::test_support::zip_test::generated_form_tsm();
    let text = String::from_utf8(bytes)
        .expect("synthetic TSM")
        .replace("e 6 1\n", "e 6 1\ne\n");
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(text.as_bytes(), &arena, &DecodePolicy::default())
            .expect("context");
    let mut cage = super::super::parse(&ctx, "synthetic.tsm", text.as_bytes())
        .expect("source cage")
        .surface
        .cage;
    cage.edit_edges(|edges| {
        edges[0].user_knot = Some(SubdUserKnot::Override(
            NonNegativeReal::new(0.0).expect("zero"),
        ));
        edges[1].user_knot = Some(SubdUserKnot::Override(
            NonNegativeReal::new(1.0).expect("one"),
        ));
        Ok(())
    })
    .expect("user edits");
    let encoded =
        super::rewrite(&ctx, "synthetic.tsm", text.as_bytes(), &cage).expect("regenerate knots");
    let rendered = std::str::from_utf8(&encoded).expect("TSM text");
    let lines: Vec<_> = rendered
        .lines()
        .filter(|line| line.starts_with("106ek "))
        .collect();
    assert_eq!(
        lines,
        ["106ek 0", "106ek 1", "106ek -1", "106ek -1", "106ek -1"]
    );
    assert_eq!(
        super::super::parse(&ctx, "synthetic.tsm", &encoded)
            .expect("read knots")
            .surface
            .cage,
        cage
    );
    cage.edit_edges(|edges| {
        for edge in edges {
            edge.user_knot = None;
        }
        Ok(())
    })
    .expect("remove extension");
    let removed =
        super::rewrite(&ctx, "synthetic.tsm", &encoded, &cage).expect("remove knot block");
    assert!(!std::str::from_utf8(&removed)
        .expect("TSM text")
        .contains("106ek"));
    assert_eq!(removed, text.as_bytes());
}
