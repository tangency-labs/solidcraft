//! Bounded SolidWorks assembly structure and reference resolution.
//! Geometry is read independently; file paths from a document are never opened here.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;

use quick_xml::Reader;
use quick_xml::events::Event;

const MAX_XML: usize = 8 << 20;
const MAX_NODES: usize = 10_000;
const IDENTITY: [[f64; 4]; 4] = [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [0., 0., 0., 1.]];

#[derive(Clone, Debug, PartialEq)]
pub struct AssemblyNode {
    pub name: String,
    /// A sibling reference, stripped of the source machine's absolute path.
    pub file: String,
    pub configuration: String,
    /// Source part configuration identity and independently stored bounds, when present.
    pub source_index: Option<u32>,
    pub bounds_mm: Option<[[f64; 3]; 2]>,
    /// Rigid column-major placement, millimetres, relative to the parent.
    pub transform: [[f64; 4]; 4],
    pub hidden: bool,
    pub children: Vec<AssemblyNode>,
}

#[derive(Default)]
struct Model {
    file: String,
    configuration: String,
    source_index: Option<u32>,
    bounds_mm: Option<[[f64; 3]; 2]>,
    references: Vec<BTreeMap<String, String>>,
}

/// Read one CRC-checked container section without decoding unrelated history or display data.
pub(super) fn section(bytes: &[u8], name: &str, limit: usize) -> Result<Option<Vec<u8>>, String> {
    if bytes.len() > crate::MAX_STEP_BYTES {
        return Err("SLDASM: input size limit exceeded".into());
    }
    let marker = [0x14, 0, 6, 0, 8, 0];
    let mut result = None;
    for (at, candidate) in bytes.windows(marker.len()).enumerate() {
        if candidate != marker {
            continue;
        }
        let word = |offset: usize| -> Option<u32> { Some(u32::from_le_bytes(bytes.get(at + offset..at + offset + 4)?.try_into().ok()?)) };
        let (Some(crc), Some(compressed), Some(size), Some(n)) = (word(10), word(14), word(18), word(22)) else { continue };
        let (compressed, size, n) = (compressed as usize, size as usize, n as usize);
        if n != name.len() {
            continue;
        }
        let Some(encoded) = bytes.get(at + 26..at + 26 + n) else { continue };
        if !encoded.iter().zip(name.as_bytes()).all(|(a, b)| a.rotate_left(4) == *b) {
            continue;
        }
        // The same marker/name also appears in cache index cells. Their three
        // length words are L*2, L/2, L, not CRC/compressed/raw lengths.
        if size > 0 && size.checked_mul(2) == Some(crc as usize) && compressed == size / 2 {
            continue;
        }
        if result.is_some() {
            return Err(format!("SLDASM: duplicate section {name}"));
        }
        if size > limit || compressed > crate::MAX_STEP_BYTES {
            return Err(format!("SLDASM: section {name} exceeds size limit"));
        }
        let data = bytes.get(at + 26 + n..at + 26 + n + compressed).ok_or("SLDASM: truncated section")?;
        let mut decoder = flate2::bufread::DeflateDecoder::new(data);
        let mut raw = Vec::new();
        decoder.by_ref().take((limit + 1) as u64).read_to_end(&mut raw).map_err(|e| format!("SLDASM: section decompression: {e}"))?;
        if raw.len() != size || decoder.total_in() != compressed as u64 || crc32fast::hash(&raw) != crc {
            return Err(format!("SLDASM: invalid size or CRC for {name}"));
        }
        result = Some(raw);
    }
    Ok(result)
}

fn sibling(path: &str) -> Result<String, String> {
    let name = path.rsplit(['/', '\\']).next().unwrap_or_default();
    if name.is_empty() || name.len() > 512 || name.contains([':', '\0']) || name == "." || name == ".." {
        return Err("SLDASM: invalid reference file name".into());
    }
    if !["sldprt", "sldasm", "step", "stp"].iter().any(|x| name.rsplit_once('.').is_some_and(|(_, e)| e.eq_ignore_ascii_case(x))) {
        return Err(format!("SLDASM: unsupported reference {name}"));
    }
    Ok(name.into())
}

fn bounds(text: Option<&String>) -> Result<Option<[[f64; 3]; 2]>, String> {
    let Some(text) = text else { return Ok(None) };
    let v = text.split_whitespace().map(str::parse::<f64>).collect::<Result<Vec<_>, _>>().map_err(|_| "SLDASM: invalid model bounds")?;
    let [a, b, c, d, e, f] = v.as_slice() else { return Err("SLDASM: invalid model bounds".into()) };
    let result = [[*a * 1000., *b * 1000., *c * 1000.], [*d * 1000., *e * 1000., *f * 1000.]];
    if result.iter().flatten().any(|v| !v.is_finite() || v.abs() > 1e9) || (0..3).any(|i| result[0][i] > result[1][i]) {
        return Err("SLDASM: invalid model bounds".into());
    }
    Ok(Some(result))
}

fn source_index(text: Option<&String>) -> Result<Option<u32>, String> {
    text.map(|text| {
        if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
            return Err("SLDASM: invalid configuration index".into());
        }
        text.parse().map_err(|_| "SLDASM: invalid configuration index".into())
    })
    .transpose()
}

fn matrix(text: Option<&String>) -> Result<[[f64; 4]; 4], String> {
    let Some(text) = text else { return Ok(IDENTITY) };
    let values = text.split_whitespace().map(str::parse::<f64>).collect::<Result<Vec<_>, _>>().map_err(|_| "SLDASM: invalid transform")?;
    if values.len() != 16 || values.iter().any(|v| !v.is_finite() || v.abs() > 1e9) {
        return Err("SLDASM: invalid transform".into());
    }
    let mut m = IDENTITY;
    for (col, chunk) in m.iter_mut().zip(values.as_chunks::<4>().0) {
        col.copy_from_slice(chunk);
    }
    let dot = |a: usize, b: usize| (0..3).map(|i| m[a][i] * m[b][i]).sum::<f64>();
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[1][0] * (m[0][1] * m[2][2] - m[0][2] * m[2][1])
        + m[2][0] * (m[0][1] * m[1][2] - m[0][2] * m[1][1]);
    if (0..3).any(|i| (0..3).any(|j| (dot(i, j) - f64::from(i == j)).abs() > 1e-6))
        || (det - 1.).abs() > 1e-6
        || (0..3).any(|i| m[i][3].abs() > 1e-12)
        || (m[3][3] - 1.).abs() > 1e-12
    {
        return Err("SLDASM: placement must be a rigid rotation and translation".into());
    }
    for value in m[3].iter_mut().take(3) {
        *value *= 1000.;
    }
    Ok(m)
}

/// Read the selected assembly's component tree. Suppressed occurrences are omitted.
/// References stay local names for the caller's explicit resolver; arbitrary original paths
/// and XML entity declarations cannot initiate filesystem or network reads.
pub fn plan(bytes: &[u8], configuration: Option<&str>) -> Result<AssemblyNode, String> {
    let xml = section(bytes, "swXmlContents/COMPINSTANCETREE", MAX_XML)?.ok_or("SLDASM: component instance tree is missing")?;
    let mut reader = Reader::from_reader(xml.as_slice());
    reader.config_mut().trim_text(true);
    let mut files = BTreeMap::new();
    let mut models: BTreeMap<String, Model> = BTreeMap::new();
    let mut configs = Vec::new();
    let mut current = None;
    let mut count = 0;
    let mut depth = 0usize;
    loop {
        let event = reader.read_event().map_err(|e| format!("SLDASM: component XML: {e}"))?;
        let is_empty = matches!(&event, Event::Empty(_));
        match event {
            Event::Start(e) | Event::Empty(e) => {
                count += 1;
                if count > MAX_NODES {
                    return Err("SLDASM: XML element limit exceeded".into());
                }
                if !is_empty {
                    depth += 1;
                    if depth > 64 {
                        return Err("SLDASM: XML depth limit exceeded".into());
                    }
                }
                let tag = e.local_name();
                let mut a = BTreeMap::new();
                for attribute in e.attributes() {
                    let attribute = attribute.map_err(|e| e.to_string())?;
                    let key = std::str::from_utf8(attribute.key.as_ref()).map_err(|e| e.to_string())?.to_string();
                    let value = attribute
                        .decoded_and_normalized_value(quick_xml::XmlVersion::Implicit1_0, reader.decoder())
                        .map_err(|e| e.to_string())?
                        .into_owned();
                    if key.len() > 128 || value.len() > 8192 || a.len() > 64 {
                        return Err("SLDASM: XML attribute limit exceeded".into());
                    }
                    if a.insert(key, value).is_some() {
                        return Err("SLDASM: duplicate XML attribute".into());
                    }
                }
                let get = |key: &str| a.get(key).cloned().ok_or_else(|| format!("SLDASM: missing {key}"));
                match tag.as_ref() {
                    b"swFile" => {
                        let id = get("id")?;
                        let name = sibling(&get("swPath")?)?;
                        if files.insert(id, name).is_some() {
                            return Err("SLDASM: duplicate file id".into());
                        }
                    }
                    b"swModel" => {
                        let id = get("id")?;
                        let model = Model {
                            file: get("swFileRef")?,
                            configuration: get("swConfigurationName")?,
                            source_index: source_index(a.get("swConfigurationId"))?,
                            bounds_mm: bounds(a.get("swBoundingBox"))?,
                            references: Vec::new(),
                        };
                        if models.insert(id.clone(), model).is_some() {
                            return Err("SLDASM: duplicate model id".into());
                        }
                        if !is_empty {
                            current = Some(id);
                        }
                    }
                    b"swReference" => {
                        let model = current.as_ref().and_then(|id| models.get_mut(id)).ok_or("SLDASM: occurrence outside a model")?;
                        model.references.push(a);
                    }
                    b"swConfiguration" => configs.push(a),
                    _ => {}
                }
            }
            Event::End(e) => {
                depth = depth.checked_sub(1).ok_or("SLDASM: malformed XML nesting")?;
                if e.local_name().as_ref() == b"swModel" {
                    current = None;
                }
            }
            Event::DocType(_) | Event::GeneralRef(_) => return Err("SLDASM: XML entity declarations are not supported".into()),
            Event::Eof => break,
            _ => {}
        }
    }
    if depth != 0 {
        return Err("SLDASM: unclosed XML element".into());
    }
    let selected: Vec<_> = configs
        .iter()
        .filter(|c| match configuration {
            Some(name) => c.get("swName").is_some_and(|n| n == name),
            None => c.get("swMostRecentConfiguration").is_some_and(|v| v == "YES"),
        })
        .collect();
    let [selected] = selected.as_slice() else { return Err("SLDASM: selected configuration is missing or ambiguous".into()) };
    let root = selected.get("swModelRef").ok_or("SLDASM: selected root is missing")?;
    let mut stack = BTreeSet::new();
    let mut budget = MAX_NODES;
    expand(root, &files, &models, &mut stack, &mut budget, 0)
}

fn expand(
    id: &str,
    files: &BTreeMap<String, String>,
    models: &BTreeMap<String, Model>,
    stack: &mut BTreeSet<String>,
    budget: &mut usize,
    depth: usize,
) -> Result<AssemblyNode, String> {
    if depth > 32 || *budget == 0 {
        return Err("SLDASM: occurrence traversal limit exceeded".into());
    }
    *budget -= 1;
    if !stack.insert(id.to_string()) {
        return Err("SLDASM: cyclic component reference".into());
    }
    let m = models.get(id).ok_or("SLDASM: missing referenced model")?;
    let file = files.get(&m.file).ok_or("SLDASM: missing referenced file")?.clone();
    let mut children = Vec::new();
    for r in &m.references {
        if r.get("swSuppressed").is_some_and(|v| v == "YES") {
            continue;
        }
        if r.get("swFlexible").is_some_and(|v| v == "YES") {
            return Err("SLDASM: flexible assembly needs an evaluated placement".into());
        }
        let mut child = expand(r.get("swModelRef").ok_or("SLDASM: occurrence model is missing")?, files, models, stack, budget, depth + 1)?;
        child.transform = matrix(r.get("swTransform"))?;
        child.hidden = r.get("swHidden").is_some_and(|v| v == "YES");
        child.name = format!("{}-{}", r.get("swName").map_or("Component", String::as_str), r.get("swReferenceNumber").map_or(id, String::as_str));
        children.push(child);
    }
    stack.remove(id);
    Ok(AssemblyNode {
        name: file.rsplit_once('.').map_or(file.clone(), |(s, _)| s.into()),
        file,
        configuration: m.configuration.clone(),
        source_index: m.source_index,
        bounds_mm: m.bounds_mm,
        transform: IDENTITY,
        hidden: false,
        children,
    })
}

/// Assemble explicitly resolved STEP parts. A resolver receives the exact configuration;
/// it must never substitute the active configuration for a different requested one.
/// Geometry is loaded once per (file, configuration), while every occurrence keeps its name
/// and placement. Closed, complete B-reps and their body/face colors are required.
pub fn restate<F>(root: &AssemblyNode, mut resolve: F) -> Result<String, String>
where
    F: FnMut(&str, &str) -> Result<String, String>,
{
    restate_nodes(root, |node| resolve(&node.file, &node.configuration), false)
}
pub(super) fn restate_nodes<F>(root: &AssemblyNode, mut resolve: F, preserve_source: bool) -> Result<String, String>
where
    F: FnMut(&AssemblyNode) -> Result<String, String>,
{
    use solidcraft_kernel::{ExportBody, ExportProduct, StepHeader, StepImport};
    struct Product {
        name: String,
        part: Option<std::sync::Arc<StepImport>>,
        source: Option<String>,
        children: Vec<(usize, [[f64; 4]; 4], String)>,
        solids: usize,
    }
    struct Build<'a, F> {
        resolve: &'a mut F,
        products: Vec<Product>,
        loaded: BTreeMap<(String, String), (usize, Option<u32>, Option<[[f64; 3]; 2]>)>,
        bytes: usize,
        nodes: usize,
        preserve_source: bool,
    }
    impl<F: FnMut(&AssemblyNode) -> Result<String, String>> Build<'_, F> {
        fn add(&mut self, n: &AssemblyNode, depth: usize) -> Result<usize, String> {
            self.nodes += 1;
            if self.nodes > MAX_NODES || depth > 32 {
                return Err("SLDASM: assembly traversal limit exceeded".into());
            }
            if n.hidden {
                return Err(format!("SLDASM: visibility for hidden occurrence {} must be resolved explicitly", n.name));
            }
            let key = (n.file.to_lowercase(), n.configuration.clone());
            if let Some((index, source_index, bounds)) = self.loaded.get(&key) {
                let same_bounds = match (bounds, &n.bounds_mm) {
                    (None, None) => true,
                    (Some(a), Some(b)) => a.iter().flatten().zip(b.iter().flatten()).all(|(a, b)| (a - b).abs() <= 1e-6),
                    _ => false,
                };
                if *source_index != n.source_index || !same_bounds {
                    return Err("SLDASM: reused part has conflicting configuration or dimensions".into());
                }
                return Ok(*index);
            }
            let index = self.products.len();
            self.products.push(Product {
                name: format!("{} [{}]", n.name, n.configuration),
                part: None,
                source: None,
                children: Vec::new(),
                solids: 0,
            });
            if n.children.is_empty() {
                if n.file.to_ascii_lowercase().ends_with(".sldasm") {
                    return Err(format!("SLDASM: unresolved child assembly {}", n.file));
                }
                let text = (self.resolve)(n)?;
                self.bytes =
                    self.bytes.checked_add(text.len()).filter(|x| *x <= 512 << 20).ok_or("SLDASM: cumulative reference size limit exceeded")?;
                if text.len() > crate::MAX_STEP_BYTES {
                    return Err("SLDASM: referenced STEP exceeds size limit".into());
                }
                let part = solidcraft_kernel::step_import_shared(&text).map_err(|e| format!("{} [{}]: {e}", n.file, n.configuration))?;
                if part.bodies.is_empty() || !part.warnings.is_empty() {
                    return Err(format!("SLDASM: {} [{}] has incomplete solids: {}", n.file, n.configuration, part.warnings.join("; ")));
                }
                // The STEP reader has checked connected, manifold, consistently closed
                // solid shells. Source packing also checks declared solid/surface kinds.
                // Fine meshing and mass properties remain deferred, as for STEP.
                if let Some((i, body)) = part.bodies.iter().enumerate().find(|(_, b)| !b.closed && !self.preserve_source) {
                    return Err(format!("SLDASM: {} [{}] solid {i} ({}) is not closed", n.file, n.configuration, body.name));
                }
                log::debug!("SLDASM: {} [{}], {} B-rep bodies", n.file, n.configuration, part.bodies.len());
                let product = self.products.get_mut(index).ok_or("SLDASM: missing product")?;
                product.solids = part.bodies.len();
                product.part = Some(part);
                product.source = Some(text);
            } else {
                let mut children = Vec::new();
                for c in &n.children {
                    // Validate public plans too, not only those read from XML.
                    let mut values = c.transform;
                    for x in values[3].iter_mut().take(3) {
                        *x /= 1000.;
                    }
                    let text = values.iter().flatten().map(ToString::to_string).collect::<Vec<_>>().join(" ");
                    let transform = matrix(Some(&text))?;
                    let child = self.add(c, depth + 1)?;
                    children.push((child, transform, c.name.clone()));
                }
                let solids = children.iter().try_fold(0usize, |total, (child, _, _)| {
                    let count = self.products.get(*child).ok_or("SLDASM: missing child product")?.solids;
                    total.checked_add(count).filter(|v| *v <= 20_000).ok_or("SLDASM: expanded solid count exceeds limit")
                })?;
                let product = self.products.get_mut(index).ok_or("SLDASM: missing assembly product")?;
                product.solids = solids;
                product.children = children;
            }
            self.loaded.insert(key, (index, n.source_index, n.bounds_mm));
            Ok(index)
        }
    }
    let mut build = Build { resolve: &mut resolve, products: Vec::new(), loaded: BTreeMap::new(), bytes: 0, nodes: 0, preserve_source };
    let root_index = build.add(root, 0)?;
    let products: Vec<_> = build
        .products
        .iter()
        .map(|p| ExportProduct {
            name: p.name.clone(),
            bodies: p
                .part
                .iter()
                .flat_map(|i| &i.bodies)
                .map(|b| ExportBody { name: b.name.clone(), body: &b.body, color: b.body.color() })
                .collect(),
            children: p.children.clone(),
        })
        .collect();
    let header = StepHeader { file_name: root.file.clone(), description: "SolidCraft SolidWorks assembly import".into(), ..Default::default() };
    let result = if preserve_source {
        let sources: Vec<_> = build.products.iter().map(|p| p.source.as_deref()).collect();
        solidcraft_kernel::step_export_products_with_sources(&products, root_index, &header, &sources)
    } else {
        solidcraft_kernel::step_export_products(&products, root_index, &header)
    };
    let text = result.map_err(|e| e.to_string())?;
    if text.len() > crate::MAX_STEP_BYTES {
        return Err("SLDASM: assembled STEP exceeds size limit".into());
    }
    Ok(text)
}

/// Restate the exact declared part configuration used by an assembly occurrence.
/// Unrelated feature-history blocks are not decoded; selected geometry and display
/// bytes must pass their original CRCs. The active configuration is never substituted.
pub fn part_to_step(bytes: &[u8], file: &str, configuration: &str) -> Result<(String, Vec<String>), String> {
    solidcraft_kernel::guard("SLDASM part import", || {
        super::convert::to_step_configuration(file, bytes, configuration).map_err(solidcraft_kernel::KernelError::Invalid)
    })
    .map_err(|e| e.to_string())
}

// A regeneration flag cannot authorize a differently sized stale configuration.
// Check its saved static solid against the assembly's independent part bounds.
pub(super) fn check_saved_bounds(text: &str, expected: [[f64; 3]; 2]) -> Result<(), String> {
    use solidcraft_geom::Aabb3;
    let part = solidcraft_kernel::step_import_shared(text).map_err(|e| e.to_string())?;
    if part.bodies.is_empty() || !part.warnings.is_empty() || part.bodies.iter().any(|b| !b.closed) {
        return Err("SLDASM: saved configuration is not a complete solid".into());
    }
    let mut bounds = Aabb3::EMPTY;
    for body in &part.bodies {
        let mass = solidcraft_kernel::measure_mass(&body.body).map_err(|e| e.to_string())?;
        if !mass.volume.is_finite() || mass.volume <= 0. {
            return Err("SLDASM: saved configuration has no positive solid volume".into());
        }
        bounds = bounds.union(&mass.bbox);
    }
    let actual = [[bounds.min.x, bounds.min.y, bounds.min.z], [bounds.max.x, bounds.max.y, bounds.max.z]];
    let size = expected[0].iter().zip(expected[1]).map(|(a, b)| (b - a).powi(2)).sum::<f64>().sqrt();
    let tolerance = 1e-4_f64.max(size * 1e-6);
    if actual.iter().flatten().zip(expected.iter().flatten()).any(|(a, b)| !a.is_finite() || (a - b).abs() > tolerance) {
        return Err("SLDASM: saved configuration dimensions disagree with the assembly".into());
    }
    Ok(())
}

/// Read referenced parts from the assembly's own directory. Absolute source-machine paths
/// are ignored; missing, duplicate or non-file sibling references remain explicit errors.
pub(crate) fn from_files(bytes: &[u8], file: &str) -> Result<(String, Vec<String>), String> {
    let root = plan(bytes, None)?;
    let parent = std::path::Path::new(file).parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| std::path::Path::new("."));
    let mut files = BTreeMap::new();
    let directory = std::fs::read_dir(parent).map_err(|e| format!("SLDASM: reference directory: {e}"))?;
    for (i, entry) in directory.enumerate() {
        if i >= 4096 {
            return Err("SLDASM: reference directory exceeds limit".into());
        }
        let entry = entry.map_err(|e| e.to_string())?;
        if !entry.file_type().map_err(|e| e.to_string())?.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if !name.ends_with(".sldprt") {
            continue;
        }
        if files.insert(name, entry.path()).is_some() {
            return Err("SLDASM: ambiguous case-insensitive reference names".into());
        }
    }
    let mut inputs = BTreeMap::<String, Vec<u8>>::new();
    let mut total = 0usize;
    let mut warnings = BTreeSet::new();
    let text = restate_nodes(
        &root,
        |node| {
            let key = node.file.to_lowercase();
            if !inputs.contains_key(&key) {
                let path = files.get(&key).ok_or_else(|| format!("SLDASM: missing referenced part {}", node.file))?;
                let length = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
                if length > crate::MAX_STEP_BYTES as u64 {
                    return Err("SLDASM: referenced part exceeds size limit".into());
                }
                let input = std::fs::read(path).map_err(|e| e.to_string())?;
                total = total.checked_add(input.len()).filter(|v| *v <= 512 << 20).ok_or("SLDASM: reference input budget exceeded")?;
                inputs.insert(key.clone(), input);
            }
            let input = inputs.get(&key).ok_or("SLDASM: missing referenced input")?;
            let index = node.source_index.ok_or("SLDASM: source configuration number is missing")?;
            if node.bounds_mm.is_none() {
                return Err("SLDASM: independent part dimensions are missing".into());
            }
            let selection = super::selection::select_stored(input, &node.configuration)?;
            if selection.source_index != index {
                return Err(format!("SLDASM: configuration identity mismatch for {}", node.file));
            }
            let (text, notes) = super::parasolid::part(input, &node.file, &selection)?;
            warnings.extend(notes);
            if selection.needs_update {
                check_saved_bounds(&text, node.bounds_mm.ok_or("SLDASM: saved configuration needs independent dimensions")?)?;
                warnings.insert(format!(
                    "SLDASM: {} [{}] uses its saved solid; the source marks its feature history for regeneration",
                    node.file, node.configuration
                ));
            }
            Ok(text)
        },
        true,
    )?;
    warnings.insert("SLDASM: imported saved solids, placements and colors; mates and parametric feature history are not imported".into());
    Ok((text, warnings.into_iter().collect()))
}
