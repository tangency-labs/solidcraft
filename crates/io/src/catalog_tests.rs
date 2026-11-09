use super::*;
use serde_json::{Value, json};
use solidcraft_doc::{pbr_appearance::PbrParameters, physical_material::ValidatedCard};

fn catalog() -> Catalog {
    let entry = Entry::new(
        "user.test",
        Definition::Appearance(LibraryLook { name: "Warm oak colour".into(), color: [180, 120, 70], opacity: 0.8, pbr: None }),
        Provenance::default(),
    )
    .unwrap();
    Catalog { format: FORMAT.into(), id: "user.test".into(), name: "Original test library".into(), entries: vec![entry] }
}
#[test]
fn catalog_roundtrip_is_deterministic_and_revision_covers_provenance_and_definition() {
    let original = catalog();
    let bytes = original.encode().unwrap();
    let restored = Catalog::decode(&bytes).unwrap();
    assert_eq!(original, restored);
    assert_eq!(bytes, restored.encode().unwrap());
    let mut changed = restored.clone();
    changed.entries[0].provenance.license = "CC0-1.0".into();
    assert!(changed.validate().is_err());
    changed.entries[0].revision = changed.entries[0].content_revision().unwrap();
    assert_ne!(changed.entries[0].revision, original.entries[0].revision);
    changed.validate().unwrap();
    if let Definition::Appearance(look) = &mut changed.entries[0].definition {
        look.color = [1, 2, 3];
    }
    assert!(changed.validate().is_err());
}
#[test]
fn strict_schema_rejects_unknown_kinds_fields_versions_and_bad_identity() {
    let value = serde_json::to_value(catalog()).unwrap();
    for changed in [
        {
            let mut v = value.clone();
            v["format"] = json!("solidcraft-library/99");
            v
        },
        {
            let mut v = value.clone();
            v["entries"][0]["definition"]["kind"] = json!("texture_set");
            v
        },
        {
            let mut v = value.clone();
            v["entries"][0]["definition"]["value"]["roughness"] = json!(0.5);
            v
        },
        {
            let mut v = value.clone();
            v["entries"][0]["id"] = json!("other.pack/appearance-a");
            v
        },
        {
            let mut v = value.clone();
            v["entries"][0]["revision"] = json!("0".repeat(64));
            v
        },
        {
            let mut v = value.clone();
            v["id"] = json!("USER.TEST");
            v
        },
        {
            let mut v = value.clone();
            v["unknown"] = json!(true);
            v
        },
    ] {
        assert!(Catalog::decode(&serde_json::to_vec(&changed).unwrap()).is_err());
    }
}
#[test]
fn scalar_validation_refuses_nonfinite_density_opacity_counts_strings_and_oversized_input() {
    let mut invalid = catalog();
    invalid.entries.push(invalid.entries[0].clone());
    assert!(invalid.validate().is_err());
    invalid.entries = vec![catalog().entries[0].clone(); MAX_ENTRIES + 1];
    assert!(invalid.validate().is_err());
    assert!(Catalog::decode(&serde_json::to_vec(&invalid).unwrap()).is_err());
    let entry = catalog().entries.remove(0);
    let mut boundary = catalog();
    boundary.entries = (0..MAX_ENTRIES).map(|i| Entry { id: format!("user.test/entry-{i}"), ..entry.clone() }).collect();
    Catalog::decode(&boundary.encode().unwrap()).unwrap();
    boundary.entries.push(Entry { id: "user.test/one-more".into(), ..entry });
    assert!(Catalog::decode(&serde_json::to_vec(&boundary).unwrap()).is_err());
    for opacity in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
        let mut entry = catalog().entries.remove(0);
        if let Definition::Appearance(look) = &mut entry.definition {
            look.opacity = opacity;
        }
        assert!(entry.content_revision().is_err());
    }
    for density in [0.0, -1.0, 1000.1, f64::NAN, f64::INFINITY] {
        assert!(
            Entry::new(
                "user.test",
                Definition::Material(LibraryMaterial { name: "Timber".into(), density_g_cm3: density, appearance: None, physical_card: None }),
                Provenance::default()
            )
            .is_err()
        );
    }
    for name in ["".into(), "x".repeat(129), "bad\nname".into(), " leading".into()] {
        assert!(
            Entry::new("user.test", Definition::Appearance(LibraryLook { name, color: [1, 2, 3], opacity: 1.0, pbr: None }), Provenance::default())
                .is_err()
        );
    }
    assert!(Catalog::decode(&vec![b' '; MAX_BYTES + 1]).is_err());
    let source = Provenance { source_url: Some("x".repeat(2049)), ..Default::default() };
    assert!(Entry::new("user.test", catalog().entries[0].definition.clone(), source).is_err());
}
#[test]
fn design_export_deduplicates_exact_looks_and_keeps_physical_properties_distinct() {
    let mut doc = Document::default();
    let look = LibraryLook { name: "Warm oak colour".into(), color: [180, 120, 70], opacity: 0.8, pbr: None }.to_look();
    doc.appearances.bodies.insert("Body1".into(), look.clone());
    doc.appearances.bodies.insert("Body2".into(), look.clone());
    doc.user_materials.push(UserMaterial { name: "Timber density".into(), density: 0.7, appearance: Some(look), physical_card: None });
    let library = Catalog::from_document(&doc, "user.test", "Test", Provenance::default()).unwrap();
    assert_eq!(library.entries.len(), 2);
    assert!(library.entries.iter().any(|e| matches!(&e.definition,Definition::Material(m) if m.density_g_cm3==0.7)));
    assert_eq!(doc.user_materials[0].density, 0.7);
    assert_eq!(library, Catalog::decode(&library.encode().unwrap()).unwrap());
    assert!(Catalog::from_document(&Document::default(), "user.test", "Test", Provenance { license: String::new(), ..Default::default() }).is_err());
}

#[test]
fn scalar_export_refuses_pbr_in_every_design_owned_appearance_location() {
    use solidcraft_doc::{appearance::FaceLook, pbr_appearance::PbrParameters};
    let mut look = LibraryLook { name: "Original PBR specimen".into(), color: [180, 120, 70], opacity: 1.0, pbr: None }.to_look();
    look.pbr = Some(PbrParameters::new(1.0, 0.3, 1.5).unwrap());
    for location in 0..4 {
        let mut doc = Document::default();
        match location {
            0 => {
                doc.appearances.bodies.insert("Specimen".into(), look.clone());
            }
            1 => {
                doc.appearances.components.insert(1, look.clone());
            }
            2 => doc.appearances.faces.push(FaceLook {
                body: "Specimen".into(),
                point: Default::default(),
                look: look.clone(),
                name: None,
                strict_name: false,
                certificate: None,
            }),
            _ => doc.user_materials.push(UserMaterial {
                name: "Original material".into(),
                density: 1.2,
                appearance: Some(look.clone()),
                physical_card: None,
            }),
        }
        let before = doc.clone();
        let rich = Catalog::from_document(&doc, "user.test", "Test", Provenance::default()).unwrap();
        assert_eq!(rich, Catalog::decode(&rich.encode().unwrap()).unwrap());
        let error = rich.in_format(V1_FORMAT).unwrap_err();
        assert!(error.to_string().contains("cannot preserve PBR"));
        assert_eq!(doc, before);
    }
}

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

fn complete_card_value() -> serde_json::Value {
    let mut data = vec![
        json!({"kind":"density","kg_m3":750.1234567890123}),
        json!({"kind":"elasticity","model":{"symmetry":"orthotropic","axes":"wood_lrt",
            "young_moduli_pa":[12e9,12e9,12e9],"shear_moduli_pa":[5e9,5e9,5e9],"poisson_ratios":[0.2,0.2,0.2]}}),
        json!({"kind":"tensile_strength","pa":12345678.901234567}),
        json!({"kind":"compressive_strength","pa":23456789.01234567}),
        json!({"kind":"yield_strength","pa":34567890.12345678}),
        json!({"kind":"shear_strength","pa":45678901.23456789}),
        json!({"kind":"thermal_conductivity","w_m_k":[1.23456789012345,2.34567890123456,3.45678901234567],"axes":"material123"}),
        json!({"kind":"thermal_expansion","per_k":[-1.234567890123e-6,2.345678901234e-6,3.456789012345e-6],"axes":"wood_lrt"}),
        json!({"kind":"specific_heat","j_kg_k":1234.5678901234567}),
    ];
    let properties: Vec<_> = data
        .drain(..)
        .enumerate()
        .map(|(i, data)| {
            json!({"source_id":if i%2==0 {"author"} else {"analytic"},
        "conditions":{"temperature_k":293.15123456789,"moisture_dry_basis_kg_kg":0.1234567890123456,"strain_rate_per_s":0.0012345678901234567,
            "state":" Original synthetic fixture "},"data":data})
        })
        .collect();
    json!({"schema_version":1,"designation":"Original complete synthetic card","family":"wood",
        "sources":[
            {"id":"author","title":"Original illustrative authoring","kind":"illustrative","license":"MIT OR Apache-2.0","reference":"fixture:original"},
            {"id":"analytic","title":"Original analytic constants","kind":"user_supplied","license":null,"reference":null},
            {"id":"retained","title":"Unused original source retained exactly","kind":"illustrative","license":"MIT","reference":"fixture:unused"}],
        "properties":properties})
}

fn card() -> ValidatedCard {
    ValidatedCard::from_value(&complete_card_value()).unwrap()
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
    let scalar_material = LibraryMaterial {
        name: "Scalar candidate".into(),
        density_g_cm3: 0.8,
        appearance: Some(LibraryLook { name: "Scalar default".into(), color: [1, 2, 3], opacity: 1.0, pbr: None }),
        physical_card: None,
    };
    let mut legacy_material = catalog().in_format(V1_FORMAT).unwrap();
    legacy_material.entries = vec![Entry::new("user.test", Definition::Material(scalar_material), Default::default()).unwrap()];
    for nested in [false, true] {
        let mut bad = serde_json::to_value(&legacy_material).unwrap();
        if nested {
            bad["entries"][0]["definition"]["value"]["appearance"]["pbr"] = Value::Null;
        } else {
            bad["entries"][0]["definition"]["value"]["physical_card"] = Value::Null;
        }
        assert!(Catalog::decode(&serde_json::to_vec(&bad).unwrap()).unwrap_err().to_string().contains("v1 definitions"));
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
fn source_and_condition_changes_participate_in_content_revisions() {
    let c = rich();
    let original = c.entries.iter().find(|e| matches!(&e.definition, Definition::Material(_))).unwrap();
    for condition in [false, true] {
        let mut entry = original.clone();
        let Definition::Material(material) = &mut entry.definition else { panic!("material fixture") };
        let mut value = serde_json::to_value(material.physical_card.as_ref().unwrap()).unwrap();
        if condition {
            value["properties"][0]["conditions"]["temperature_k"] = json!(301.123456789);
        } else {
            value["sources"][2]["reference"] = json!("fixture:changed-unused-source");
        }
        material.physical_card = Some(ValidatedCard::from_value(&value).unwrap());
        assert_ne!(entry.content_revision().unwrap(), original.revision);
    }
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
