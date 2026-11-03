use solidcraft_geom::cell::Projection;
use solidcraft_geom::drawing_sheet::SheetSize;

use super::*;
use crate::drawing::Sheet;
use crate::drawing::tests::plate;

fn made(name: &str) -> Drawing {
    built_in(name).unwrap_or_else(|| panic!("no built-in {name}")).unwrap()
}

/// The built-in `name` with the built-in block on its sheet, an assembly's or a part's.
fn titled(name: &str, assembly: bool) -> Drawing {
    let mut d = made(name);
    Arc::make_mut(&mut d.sheets[0]).set_title_block(Some(built_in_block(assembly)));
    check(&d).unwrap();
    d
}

/// The text of a block's general tolerances cell.
fn tolerances(block: &TitleBlock) -> String {
    let cell = block.cells.iter().find(|cell| cell.name == "general_tolerances").unwrap();
    let CellKind::Text(text) = &cell.kind else { panic!("the tolerances cell shows text") };
    let [Part::Text(text)] = &text.content.0[..] else { panic!("the tolerances are plain text") };
    text.clone()
}

/// Every built-in is a template on its ISO paper (A4 upright, the rest flat) with the ISO border
/// and nothing more, the same bytes each time it is made. Another name is none.
#[test]
fn every_built_in_is_a_template_on_its_paper() {
    let mut seen = BTreeSet::new();
    assert!(BUILT_INS.contains(&DEFAULT));
    for &name in BUILT_INS {
        let d = made(name);
        check(&d).unwrap();
        assert_eq!(d.sheets.len(), 1, "{name}");
        let sheet = &d.sheets[0];
        let size = name.strip_prefix("iso-").unwrap();
        let SheetSize::Standard(paper) = sheet.sheet.size else { panic!("{name}: a standard size") };
        assert_eq!(serde_json::to_value(paper).unwrap(), serde_json::json!(size), "{name}");
        let upright = if size == "a4" { Orientation::Portrait } else { Orientation::Landscape };
        assert_eq!((sheet.sheet.orientation, &sheet.border), (upright, &Some(DrawingBorderProfile::iso())), "{name}");
        assert_eq!(sheet.title_block, None, "{name}: a border only");
        assert_eq!(d.name, format!("ISO {}", size.to_uppercase()));
        assert_eq!(title(name), Some(d.name.clone()), "{name}: its title without making it");
        assert_eq!(d.to_json().unwrap(), made(name).to_json().unwrap(), "{name}: the same bytes");
        assert!(seen.insert(d.to_json().unwrap()), "{name}: a built-in of its own");
    }
    for name in ["iso-a5", "iso-a3-part", "iso-a3-assembly", "iso-A3", "a3", ""] {
        assert!(built_in(name).is_none() && title(name).is_none(), "{name}");
    }
}

/// The built-in block is the default block with a part's or an assembly's general tolerances.
#[test]
fn the_built_in_block_takes_its_tolerances() {
    for (assembly, expected) in [(false, PART_TOLERANCES), (true, ASSEMBLY_TOLERANCES)] {
        let mut block = built_in_block(assembly);
        assert_eq!(tolerances(&block), expected);
        let default = title_block::default_block();
        let at = block.cells.iter().position(|cell| cell.name == "general_tolerances").unwrap();
        block.cells[at] = default.cells[at].clone();
        assert_eq!(block, default, "only the tolerances differ");
    }
}

/// A template is saved and read as a drawing is, and is still a template.
#[test]
fn a_template_reads_back_as_saved() {
    let mut d = titled("iso-a2", true);
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
    let mut d = titled("iso-a3", false);
    Arc::make_mut(&mut d.revisions).add(None, 77).unwrap();
    assert_eq!(check(&d).unwrap_err().to_string(), "a template draws no design, and this one has revision rows");
    // A drawing's own rules come first.
    let mut d = titled("iso-a3", false);
    d.sheets.clear();
    assert!(check(&d).unwrap_err().to_string().contains("a drawing has 1 to 1000 sheets"));
}

/// Applying a template gives each sheet the template's paper, border and block (its one sheet's to
/// both of a drawing's two), keeping the text typed over a cell the new block still has; nothing
/// changes for the same template; a drawing with a model is no template.
#[test]
fn a_template_applied_gives_each_sheet_its_format() {
    let mut d = instantiate(&titled("iso-a3", false), "Bracket drawing", &BTreeSet::new(), &mut IdSource::seeded(1)).unwrap();
    let id = d.allocate_id(&mut IdSource::seeded(2)).unwrap();
    d.sheets.push(Arc::new(Sheet { id, name: "Sheet 2".into(), ..(*d.sheets[0]).clone() }));
    for (cell, text) in [("material", "See note 3"), ("finish", "Black oxide")] {
        Arc::make_mut(&mut d.sheets[0]).title_block_overrides.insert(cell.into(), text.into());
    }
    assert_eq!(apply(&d, &titled("iso-a3", false)).unwrap(), (d.clone(), Vec::new()), "the same template changes nothing");
    let (applied, changed) = apply(&d, &titled("iso-a2", true)).unwrap();
    let ids: Vec<u64> = d.sheets.iter().map(|s| s.id).collect();
    let restyled = |sheet| Restyled { sheet, paper: true, border: false, title_block: true, lost: Vec::new() };
    assert_eq!(changed, ids.iter().map(|id| restyled(*id)).collect::<Vec<_>>());
    let a2 = titled("iso-a2", true);
    for sheet in &applied.sheets {
        assert_eq!((sheet.sheet, &sheet.title_block), (a2.sheets[0].sheet, &a2.sheets[0].title_block));
    }
    assert_eq!(applied.sheets[0].title_block_overrides, d.sheets[0].title_block_overrides, "every cell is still there");
    assert_eq!((applied.id, applied.name.as_str(), &applied.sources), (d.id, "Bracket drawing", &d.sources));
    // A block without a finish cell: its typed text is lost, and said so.
    let mut renamed = titled("iso-a3", false);
    let block = Arc::make_mut(&mut renamed.sheets[0]).title_block.as_mut().unwrap();
    block.cells.iter_mut().find(|cell| cell.name == "finish").unwrap().name = "surface".into();
    let (applied, changed) = apply(&d, &renamed).unwrap();
    assert_eq!(changed[0], Restyled { sheet: ids[0], paper: false, border: false, title_block: true, lost: vec!["finish".into()] });
    let kept: Vec<&str> = applied.sheets[0].title_block_overrides.keys().map(String::as_str).collect();
    assert_eq!(kept, ["material"]);
    assert!(apply(&d, &plate()).unwrap_err().to_string().starts_with("a template draws no design"));
}

/// A drawing saved as a template keeps its sheets and leaves its model behind, and reads back as
/// one; a drawing's own file is no template.
#[test]
fn a_drawing_saved_as_a_template_keeps_only_its_format() {
    let mut d = plate();
    d.properties = Arc::new(serde_json::from_value(serde_json::json!({"part_number": "D-1234", "project": "Rig"})).unwrap());
    let template = from_drawing(&d).unwrap();
    check(&template).unwrap();
    assert!(template.sources.is_empty() && template.views.is_empty() && template.dimensions.is_empty() && template.balloons.is_empty());
    assert!(template.sheets.iter().all(|sheet| sheet.documents.is_none()));
    let format = |sheet: &Sheet| (sheet.id, sheet.name.clone(), sheet.sheet, sheet.border, sheet.title_block.clone(), sheet.dpi);
    assert_eq!(template.sheets.iter().map(|s| format(s)).collect::<Vec<_>>(), d.sheets.iter().map(|s| format(s)).collect::<Vec<_>>());
    assert_eq!((template.projection, &template.units), (d.projection, &d.units));
    // The drawing's number is its own; its other properties come along.
    assert_eq!((template.properties.part_number(), template.properties.project()), (None, Some("Rig")));
    assert_eq!(read(&template.to_json().unwrap()).unwrap(), template);
    // Text typed over a cell of plain text stays; over a cell that shows a field, it goes with the
    // design that field reads.
    let sheet = Arc::make_mut(&mut d.sheets[0]);
    sheet.set_title_block(Some(built_in_block(false)));
    for (cell, text) in [("general_tolerances", "ISO 2768-m"), ("material", "See note 3"), ("title", "Plate, left")] {
        sheet.title_block_overrides.insert(cell.into(), text.into());
    }
    let kept: Vec<String> = from_drawing(&d).unwrap().sheets[0].title_block_overrides.keys().cloned().collect();
    assert_eq!(kept, ["general_tolerances"]);
    let refusal = read(&d.to_json().unwrap()).unwrap_err().to_string();
    assert!(refusal.starts_with("a template draws no design"), "{refusal}");
}

/// A damaged template file, cut short or with a byte changed anywhere, is read or refused, never
/// a panic.
#[test]
fn a_damaged_template_file_never_panics() {
    let text = titled("iso-a3", false).to_json().unwrap();
    let mut read_back = 0;
    for at in (0..text.len()).step_by(7) {
        let _ = text.get(..at).map(read);
        for byte in *b"\"9{-x" {
            let mut bytes = text.clone().into_bytes();
            bytes[at] = byte;
            if let Ok(changed) = String::from_utf8(bytes) {
                read_back += usize::from(read(&changed).is_ok());
            }
        }
    }
    assert!(read_back > 0, "some changes leave a template that still reads (in a name or a number)");
}

/// A drawing from a template has its own ids, none of the template's or of `taken`, the same ones
/// again from the same seed; the name it is given; and everything else of the template, the text
/// typed over its cells too.
#[test]
fn a_drawing_from_a_template_has_ids_of_its_own() {
    let mut template = titled("iso-a1", false);
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
