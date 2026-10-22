use super::*;

const QUAD: &str = r#"#VRML V2.0 utf8
Transform {
  translation 1 2 3
  rotation 0 0 1 1.5707963267948966
  children [
    Shape {
      appearance Appearance { material Material { diffuseColor 0.8 0.2 0.1 } }
      geometry IndexedFaceSet {
        coord Coordinate { point [ 0 0 0, 2 0 0, 2 1 0, 0 1 0 ] }
        coordIndex [ 0, 1, 2, -1, 0, 2, 3, -1 ]
      }
    }
  ]
}
"#;

#[test]
fn reads_a_transformed_indexed_face_set() {
    let mesh = read_vrml(QUAD.as_bytes(), "part.wrl").unwrap();
    assert_eq!(mesh.triangles.len(), 2);
    assert_eq!(mesh.color, Some([0.8, 0.2, 0.1]));
    // (0,0,0) and (2,0,0) turn a quarter turn about Z, then move by (1,2,3).
    let find = |want: Vec3| mesh.positions.iter().any(|p| (*p - want).len() < 1e-9);
    assert!(find(Vec3::new(1.0, 2.0, 3.0)), "{:?}", mesh.positions);
    assert!(find(Vec3::new(1.0, 4.0, 3.0)), "{:?}", mesh.positions);
    assert_eq!(mesh.warnings, Vec::<String>::new());
}

#[test]
fn resolves_def_use_and_counts_primitives() {
    let source = r#"#VRML V2.0 utf8
DEF box Shape { geometry Box { size 2 2 2 } }
Group { children [ USE box, Transform { children [ USE box ] } ] }
"#;
    let mesh = read_vrml(source.as_bytes(), "boxes.wrl").unwrap();
    assert_eq!(mesh.triangles.len(), 36, "the DEF node and its two USE copies, six quads each");
}

#[test]
fn refuses_vrml_1_and_unterminated_files() {
    assert!(read_vrml(b"#VRML V1.0 ascii\nSeparator {}\n", "old.wrl").is_err());
    assert!(read_vrml(b"#VRML V2.0 utf8\nShape { geometry Box {", "bad.wrl").is_err());
    assert!(!is_vrml_path("part.step"));
    assert!(is_vrml_path("part.WRL"));
}

#[test]
fn refuses_excessively_nested_lists() {
    let nesting = MAX_DEPTH + 1;
    let mut source = String::from("#VRML V2.0 utf8\nShape { geometry Box { size ");
    source.push_str(&"[".repeat(nesting));
    source.push('1');
    source.push_str(&"]".repeat(nesting));
    source.push_str(" } }\n");

    let error = read_vrml(source.as_bytes(), "nested.wrl").unwrap_err();
    assert!(error.to_string().contains("nesting exceeds"), "{error}");
}

#[test]
fn refuses_a_model_without_geometry() {
    let error = read_vrml(b"#VRML V2.0 utf8\nGroup { children [ ] }\n", "empty.wrl").unwrap_err();
    assert!(error.to_string().contains("no geometry"));
}
