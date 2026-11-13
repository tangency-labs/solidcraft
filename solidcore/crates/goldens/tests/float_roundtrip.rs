//! Generated cases and text fixtures must send bit-identical inputs to both kernels.
#[test]
fn fixture_json_preserves_generated_double_bits() {
    let inputs = [
        -0.916_536_277_098_507_5_f64,
        -0.205_850_726_168_449_46,
        103.227_956_389_191_75,
        0.607_540_057_359_745_7,
        f64::MIN_POSITIVE,
        f64::MAX,
        -0.0,
    ];
    let text = serde_json::to_string(&inputs).unwrap();
    let restored: Vec<f64> = serde_json::from_str(&text).unwrap();
    assert_eq!(inputs.len(), restored.len());
    for (before, after) in inputs.iter().zip(restored) {
        assert_eq!(before.to_bits(), after.to_bits(), "{text}");
    }
}
