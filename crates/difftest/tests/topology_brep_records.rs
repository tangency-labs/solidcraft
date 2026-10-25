//! Non-C++ regression coverage for the runner's actual native record decoder.

pub use difftest::HarnessError;

#[path = "../src/units/fixtures/topology_brep_records.rs"]
mod records;

fn assert_rejected(input: &[f64], message: &str) {
    // Panics and capacity overflows fail the test rather than count as rejection.
    let result = records::parse_records(input);
    assert!(result.is_err(), "malformed records accepted");
    if let Err(error) = result {
        let error = error.to_string();
        assert!(error.contains(message), "{error}");
    }
}

fn assert_bad_counts(value: f64) {
    for kind in 0..10 {
        let input: [f64; 10] = std::array::from_fn(|i| if i == kind { value } else { 0.0 });
        assert_rejected(&input, "bad record count");
    }
    // Minimal records for every kind with a list; indices locate list counts.
    for (kind, mut record, list_at) in [
        (0, vec![7.0, 1.0, 0.0], 2),
        (2, vec![7.0, -1.0, -1.0, -1.0, 1.0, 0.0], 5),
        (7, vec![7.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0], 4),
        (9, vec![7.0, 0.0, 0.0, 0.0, 0.0, 0.0], 5),
    ] {
        if let Some(slot) = record.get_mut(list_at) {
            *slot = value;
        }
        let mut input = vec![0.0; kind];
        input.push(1.0);
        input.extend(record);
        input.extend(vec![0.0; 9 - kind]);
        assert_rejected(&input, "bad list count");
    }
}

#[test]
fn nan_counts_are_rejected_in_every_position() {
    assert_rejected(&[f64::NAN; 10], "bad record count");
    assert_bad_counts(f64::NAN);
}

#[test]
fn infinite_counts_are_rejected_in_every_position() {
    assert_bad_counts(f64::INFINITY);
    assert_bad_counts(f64::NEG_INFINITY);
}

#[test]
fn negative_counts_are_rejected_in_every_position() {
    assert_bad_counts(-1.0);
}

#[test]
fn fractional_counts_are_rejected_in_every_position() {
    assert_bad_counts(0.5);
    assert_bad_counts(-0.5);
}

#[test]
fn exclusive_usize_limit_and_larger_counts_are_rejected() {
    assert_bad_counts(2.0_f64.powi(usize::BITS as i32));
    assert_bad_counts(1e100);
}

#[test]
fn huge_representable_record_counts_are_rejected_before_iteration() {
    let large = 2.0_f64.powi(usize::BITS as i32 - 1);
    for kind in 0..10 {
        let input: [f64; 10] = std::array::from_fn(|i| if i == kind { large } else { 0.0 });
        assert_rejected(&input, "short record output: record count");
    }
}

#[test]
fn huge_and_truncated_list_counts_are_rejected_before_iteration() {
    for count in [2.0, 2.0_f64.powi(usize::BITS as i32 - 1)] {
        assert_rejected(
            &[1.0, 7.0, 1.0, count, 9.0],
            "short record output: list count",
        );
    }
}

#[test]
fn truncated_counts_records_and_lists_are_rejected() {
    for input in [
        &[][..],
        &[0.0; 9],
        &[1.0],
        &[1.0, 7.0, 1.0],
        &[1.0, 7.0, 1.0, 0.0],
        &[1.0, 7.0, 1.0, 2.0, 9.0],
    ] {
        assert_rejected(input, "short record output");
    }
}

#[test]
fn trailing_tokens_are_rejected() {
    assert_rejected(&[0.0; 11], "trailing record output");
}

#[test]
fn empty_model_and_signed_zero_counts_are_valid() -> Result<(), HarnessError> {
    for input in [[0.0; 10], [-0.0; 10]] {
        let model = records::parse_records(&input)?;
        assert!(model.iter().all(Vec::is_empty));
    }
    Ok(())
}

#[test]
fn valid_records_preserve_labels_links_lists_order_and_float_bits() -> Result<(), HarnessError> {
    let nan = f64::from_bits(0x7ff8_0000_0000_0042);
    let mut input = vec![
        2.0, // regions, deliberately not in label order
        42.0, 1.0, 3.0, 9.0, -1.0, 2.0, 3.0, 0.0, 0.0,
    ];
    input.extend([0.0; 8]); // shells through vertex uses
    input.extend([
        1.0,
        71.0,
        -0.0,
        nan,
        f64::NEG_INFINITY,
        f64::from_bits(1),
        3.0,
        8.0,
        2.0,
        8.0,
    ]);
    let model = records::parse_records(&input)?;
    let mut region = records::Rec::new(42);
    region.i(1);
    region.l(1, &[9, -1, 2]);
    let mut empty_region = records::Rec::new(3);
    empty_region.i(0);
    empty_region.l(1, &[]);
    assert_eq!(model[0], vec![region, empty_region]);
    assert!(model[1..9].iter().all(Vec::is_empty));
    let vertex = &model[9][0];
    assert_eq!(vertex.label, 71);
    assert_eq!(vertex.ints, vec![3, 8, 2, 8]);
    assert_eq!(vertex.links, vec![None, Some(8), Some(8), Some(8)]);
    assert_eq!(
        vertex
            .floats
            .iter()
            .map(|x| x.to_bits())
            .collect::<Vec<_>>(),
        vec![
            (-0.0_f64).to_bits(),
            nan.to_bits(),
            f64::NEG_INFINITY.to_bits(),
            1
        ]
    );
    Ok(())
}
