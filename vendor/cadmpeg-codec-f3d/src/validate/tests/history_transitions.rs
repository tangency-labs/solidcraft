// SPDX-License-Identifier: Apache-2.0
//! Native history transition invariants.

#[test]
fn decoded_history_reports_an_invalid_transition() {
    crate::test_support::with_decode_context(|decode| {
        let bytes = crate::test_support::smbh_geometry_test::synthetic_geometry_with_history_smbh();
        let decoded = crate::history::decode(
            decode,
            &bytes,
            "BREP.test.smbh",
            cadmpeg_asm::kernel_header::RefWidth::Eight,
            &mut { u64::MAX },
            &cadmpeg_core::decode::WorkBudget::new(
                crate::history::binding::MAX_HISTORY_BINDING_WORK,
            ),
        )
        .unwrap()
        .unwrap();
        let states = decoded
            .states()
            .iter()
            .cloned()
            .map(|state| {
                (
                    state,
                    Some(crate::history_records::AsmHistoricalTopology::default()),
                    Vec::new(),
                )
            })
            .collect();
        let mut history = crate::test_support::history::history(
            decoded.id,
            decoded.byte_offset,
            decoded.preamble,
            false,
            states,
        );
        assert_eq!(history.check_transitions(), Ok(()));
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let mut native = crate::native::F3dNative {
            asm_histories: vec![history.clone()],
            ..Default::default()
        };
        let mut findings = Vec::new();
        super::super::validate_history_graphs(
            &super::super::Ctx::new(&ir, &native, decode),
            &mut findings,
        );
        assert!(findings.is_empty());
        history.states_mut()[0]
            .transition
            .as_mut()
            .unwrap()
            .previous_state_id = Some(-9);
        native.asm_histories = vec![history];
        super::super::validate_history_graphs(
            &super::super::Ctx::new(&ir, &native, decode),
            &mut findings,
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].check,
            cadmpeg_ir::report::check::Check::NativeLinks
        );
        assert_eq!(findings[0].severity, cadmpeg_ir::report::Severity::Error);
        assert_eq!(
            findings[0].entity.as_deref(),
            Some(native.asm_histories[0].id.as_str())
        );
        assert!(findings[0].message.contains("PreviousState"));
    });
}
