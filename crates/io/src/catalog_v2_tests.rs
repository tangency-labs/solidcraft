use super::*;
use serde_json::{Value, json};
use solidcraft_doc::{pbr_appearance::PbrParameters, physical_material::ValidatedCard};

// Exact bytes and revisions captured from the pre-v2 writer at 75e66eea.
const V1: &str = r#"{
  "format": "solidcraft-library/1",
  "id": "user.test",
  "name": "Original v1 fixture",
  "entries": [
    {
      "id": "user.test/appearance-c17adc43c1eb8c7d",
      "revision": "c17adc43c1eb8c7d6badd7e7821150509bf513eb29c75a90c4027f46d9b49437",
      "provenance": {
        "license": "LicenseRef-User-Provided"
      },
      "definition": {
        "kind": "appearance",
        "value": {
          "name": "Original scalar study",
          "color": [
            180,
            120,
            70
          ],
          "opacity": 0.8
        }
      }
    }
  ]
}"#;

fn card() -> ValidatedCard {
    ValidatedCard::from_value(&json!({"schema_version":1,"designation":"Original synthetic card","family":"other",
        "sources":[{"id":"fixture","title":"Original invented constants","kind":"illustrative"}],
        "properties":[{"source_id":"fixture","data":{"kind":"elasticity","model":{
            "symmetry":"isotropic","young_modulus_pa":12.25e9,"poisson_ratio":0.25}}}]}))
    .unwrap()
}

fn rich() -> Catalog {
    let mut doc = Document::default();
    let mut look = Look::custom([180, 120, 70], 1.0);
    look.name = "Original opaque study".into();
    look.pbr = Some(PbrParameters::new(1.0, 0.3, 1.5).unwrap());
    doc.appearances.bodies.insert("Body1".into(), look.clone());
    doc.user_materials.push(UserMaterial { name: "Original candidate".into(), density: 0.8, appearance: Some(look), physical_card: Some(card()) });
    Catalog::from_document(&doc, "user.test", "Original v2 fixture", Default::default()).unwrap()
}

#[test]
fn pinned_v1_bytes_revisions_and_scalar_material_canonical_fields_are_preserved() {
    let old = Catalog::decode(V1.as_bytes()).unwrap();
    assert_eq!(old.encode().unwrap(), V1.as_bytes());
    let upgraded = old.clone().in_format(FORMAT).unwrap();
    assert_eq!(upgraded.entries, old.entries);
    assert_eq!(upgraded.encode().unwrap(), V1.replacen(V1_FORMAT, FORMAT, 1).as_bytes());
    let material = LibraryMaterial {
        name: "Original scalar material".into(),
        density_g_cm3: 0.7,
        appearance: Some(LibraryLook { name: "Original scalar material".into(), color: [180, 120, 70], opacity: 1.0, pbr: None }),
        physical_card: None,
    };
    let provenance = Provenance {
        license: "MIT OR Apache-2.0".into(),
        author: Some("SolidCraft contributors".into()),
        source_url: Some("https://example.invalid/original".into()),
    };
    let entry = Entry::new("user.test", Definition::Material(material), provenance).unwrap();
    assert_eq!(entry.revision, "3dc003a875463baef587b85b9883f227cb546178317271c60078989bdb86c245");
    assert_eq!(entry.id, "user.test/material-3dc003a875463bae");
}

#[test]
fn v2_roundtrip_keeps_optics_cards_and_revisions_cover_richer_data() {
    let original = rich();
    let copy = Catalog::decode(&original.encode().unwrap()).unwrap();
    assert_eq!(copy, original);
    assert!(copy.clone().in_format(V1_FORMAT).is_err());
    let mut changed = copy.clone();
    for entry in &mut changed.entries {
        match &mut entry.definition {
            Definition::Appearance(l) => l.pbr = Some(PbrParameters::new(0., 0.8, 1.4).unwrap()),
            Definition::Material(m) => {
                assert_eq!(m.to_material(m.name.clone()).physical_card, Some(card()));
                let mut v = serde_json::to_value(card()).unwrap();
                v["designation"] = json!("Changed synthetic designation");
                m.physical_card = Some(ValidatedCard::from_value(&v).unwrap());
            }
        }
        assert_ne!(entry.content_revision().unwrap(), entry.revision);
    }
    assert!(changed.validate().is_err());
}

#[test]
fn wire_versions_unknown_fields_invalid_optics_and_hostile_cards_refuse() {
    let legacy: Value = serde_json::from_str(V1).unwrap();
    for value in [Value::Null, json!({"metallic":0,"roughness":0.3,"dielectric_ior":1.5})] {
        let mut bad = legacy.clone();
        bad["entries"][0]["definition"]["value"]["pbr"] = value;
        assert!(Catalog::decode(&serde_json::to_vec(&bad).unwrap()).is_err());
    }
    let v = serde_json::to_value(rich()).unwrap();
    let appearance = v["entries"].as_array().unwrap().iter().position(|e| e["definition"]["kind"] == "appearance").unwrap();
    let material = v["entries"].as_array().unwrap().iter().position(|e| e["definition"]["kind"] == "material").unwrap();
    for (path, value) in [("opacity", json!(0.5)), ("pbr", json!({"metallic":0,"roughness":0,"dielectric_ior":1.5})), ("unknown", json!(true))] {
        let mut bad = v.clone();
        bad["entries"][appearance]["definition"]["value"][path] = value;
        assert!(Catalog::decode(&serde_json::to_vec(&bad).unwrap()).is_err());
    }
    let mut bad = v.clone();
    bad["entries"][material]["definition"]["value"]["physical_card"]["sources"] = json!(vec![json!({}); 33]);
    assert!(Catalog::decode(&serde_json::to_vec(&bad).unwrap()).is_err());
    let mut downgraded = v.clone();
    downgraded["format"] = json!(V1_FORMAT);
    assert!(Catalog::decode(&serde_json::to_vec(&downgraded).unwrap()).is_err());
}

#[test]
fn rich_export_bounds_retained_definitions_before_whole_catalog_encoding() {
    let mut v = serde_json::to_value(card()).unwrap();
    v["properties"] = json!([]);
    v["sources"] = json!(
        (0..32)
            .map(|i| json!({"id":format!("s{i}"),"title":"Original synthetic metadata","kind":"illustrative","reference":"x".repeat(2048)}))
            .collect::<Vec<_>>()
    );
    let record = ValidatedCard::from_value(&v).unwrap();
    let mut doc = Document::default();
    for i in 0..64 {
        doc.user_materials.push(UserMaterial { name: format!("Candidate {i}"), density: 0.8, appearance: None, physical_card: Some(record.clone()) });
    }
    assert!(Catalog::from_document(&doc, "user.test", "Original large fixture", Default::default()).unwrap_err().to_string().contains("4 MiB"));
}
