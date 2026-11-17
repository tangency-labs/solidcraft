use super::*;
#[test]
fn workshop_svg_escapes_names_and_keeps_mm_sizes_and_pages() {
    let pages =
        [Page { width: 100.0, height: 55.0, texts: vec![Text { x: 3.0, y: 9.0, size: 6.0, value: "A-1 <Rail> & Oak".into() }], ..Page::default() }];
    let out = svg(&pages).unwrap();
    assert!(out.contains("width=\"100mm\""));
    assert_eq!(out.matches("id=\"page-").count(), 1);
    assert!(out.contains("A-1 &lt;Rail&gt; &amp; Oak"));
    let dxf = dxf(&pages).unwrap();
    assert!(dxf.contains("A-1 <Rail> & Oak"));
    assert!(svg(&[Page { width: f64::NAN, ..Page::default() }]).is_err());
}
