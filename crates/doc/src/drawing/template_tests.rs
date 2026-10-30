use solidcraft_geom::cell::Projection;
use solidcraft_geom::drawing_sheet::SheetSize;

use super::*;
use crate::drawing::Sheet;
use crate::drawing::tests::plate;

fn made(name: &str) -> Drawing {
    built_in(name).unwrap_or_else(|| panic!("no built-in {name}")).unwrap()
}

/// The text of a sheet's general tolerances cell.
fn tolerances(d: &Drawing) -> String {
    let block = d.sheets[0].title_block.as_ref().unwrap();
    let cell = block.cells.iter().find(|cell| cell.name == "general_tolerances").unwrap();
    let CellKind::Text(text) = &cell.kind else { panic!("the tolerances cell shows text") };
    let [Part::Text(text)] = &text.content.0[..] else { panic!("the tolerances are plain text") };
    text.clone()
}

/// Every built-in is a template on its ISO paper (A4 upright, the rest flat) with the ISO border
/// and the built-in block, laid out, its general tolerances by its kind, and the same bytes each
/// time it is made. Another name is none.
#[test]
fn every_built_in_is_a_template_on_its_paper() {
    let mut seen = BTreeSet::new();
    for &name in BUILT_INS {
        let d = made(name);
        check(&d).unwrap();
        assert_eq!(d.sheets.len(), 1, "{name}");
        let sheet = &d.sheets[0];
        let (size, kind) = name.strip_prefix("iso-").unwrap().split_once('-').unwrap();
        let SheetSize::Standard(paper) = sheet.sheet.size else { panic!("{name}: a standard size") };
        assert_eq!(serde_json::to_value(paper).unwrap(), serde_json::json!(size), "{name}");
        let upright = if size == "a4" { Orientation::Portrait } else { Orientation::Landscape };
        assert_eq!((sheet.sheet.orientation, &sheet.border), (upright, &Some(DrawingBorderProfile::iso())), "{name}");
        assert_eq!(d.name, format!("ISO {} {kind}", size.to_uppercase()));
        assert_eq!(title(name), Some(d.name.clone()), "{name}: its title without making it");
        let expected = if kind == "part" { PART_TOLERANCES } else { ASSEMBLY_TOLERANCES };
        assert_eq!(tolerances(&d), expected, "{name}");
        // Only the tolerances differ from the built-in block.
        let mut block = sheet.title_block.clone().unwrap();
        let default = title_block::default_block();
        let at = block.cells.iter().position(|cell| cell.name == "general_tolerances").unwrap();
        block.cells[at] = default.cells[at].clone();
        assert_eq!(block, default, "{name}");
        assert_eq!(d.to_json().unwrap(), made(name).to_json().unwrap(), "{name}: the same bytes");
        assert!(seen.insert(d.to_json().unwrap()), "{name}: a built-in of its own");
    }
    for name in ["iso-a5-part", "iso-a3-sheet", "iso-A3-part", "a3-part", "iso-a3", ""] {
        assert!(built_in(name).is_none() && title(name).is_none(), "{name}");
    }
}

/// A template is saved and read as a drawing is, and is still a template.
#[test]
fn a_template_reads_back_as_saved() {
    let mut d = made("iso-a2-assembly");
    d.projection = Projection::First;
    let saved = d.to_json().unwrap();
    let read = Drawing::from_json(&saved).unwrap();
    check(&read).unwrap();
    assert_eq!(read, d);
}

/// A drawing with anything of a model is no template: one message names all it has.
#[test]
fn a_template_names_no_model() {
    let refusal = check(&plate()).unwrap_err().to_string();
    let all = "a template draws no design, and this one has a source, views, dimensions, balloons, a sheet that documents a component";
    assert_eq!(refusal, all);
    let mut d = made("iso-a3-part");
    Arc::make_mut(&mut d.revisions).add(None, 77).unwrap();
    assert_eq!(check(&d).unwrap_err().to_string(), "a template draws no design, and this one has revision rows");
    // A drawing's own rules come first.
    let mut d = made("iso-a3-part");
    d.sheets.clear();
    assert!(check(&d).unwrap_err().to_string().contains("a drawing has 1 to 1000 sheets"));
}

/// A drawing from a template has its own ids, none of the template's or of `taken`, the same ones
/// again from the same seed; the name it is given; and everything else of the template, the text
/// typed over its cells too.
#[test]
fn a_drawing_from_a_template_has_ids_of_its_own() {
    let mut template = made("iso-a1-part");
    let first = (*template.sheets[0]).clone();
    template.sheets.push(Arc::new(Sheet { id: 5, name: "Sheet 2".into(), ..first }));
    Arc::make_mut(&mut template.sheets[1]).title_block_overrides.insert("material".into(), "See note 3".into());
    check(&template).unwrap();
    let taken = BTreeSet::from([template.id]);
    let d = instantiate(&template, "Bracket drawing", &taken, &mut IdSource::seeded(9)).unwrap();
    let ids = |d: &Drawing| std::iter::once(d.id).chain(d.sheets.iter().map(|s| s.id)).collect::<Vec<_>>();
    assert!(ids(&d).iter().all(|id| !ids(&template).contains(id) && !taken.contains(id)), "{:?} {:?}", ids(&d), ids(&template));
    assert_eq!(ids(&d), ids(&instantiate(&template, "Bracket drawing", &taken, &mut IdSource::seeded(9)).unwrap()));
    assert_ne!(ids(&d), ids(&instantiate(&template, "Bracket drawing", &taken, &mut IdSource::seeded(10)).unwrap()));
    assert_eq!(d.name, "Bracket drawing");
    let same = |d: &Drawing| {
        let sheets: Vec<Sheet> = d.sheets.iter().map(|s| Sheet { id: 0, ..(**s).clone() }).collect();
        (sheets, d.projection, d.units.clone(), d.language.clone(), d.properties.clone())
    };
    assert_eq!(same(&d), same(&template));
    check(&d).unwrap();
    // A drawing with a model makes none.
    let refusal = instantiate(&plate(), "Copy", &taken, &mut IdSource::seeded(9)).unwrap_err().to_string();
    assert!(refusal.starts_with("a template draws no design"), "{refusal}");
}
