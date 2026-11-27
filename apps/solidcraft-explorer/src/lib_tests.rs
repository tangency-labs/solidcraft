use super::*;

#[test]
fn the_class_id_text_and_number_agree() {
    let n = CLSID_U128;
    let text =
        format!("{{{:08X}-{:04X}-{:04X}-{:04X}-{:012X}}}", n >> 96, (n >> 80) & 0xFFFF, (n >> 64) & 0xFFFF, (n >> 48) & 0xFFFF, n & 0xFFFF_FFFF_FFFF);
    assert_eq!(text, CLSID);
}
