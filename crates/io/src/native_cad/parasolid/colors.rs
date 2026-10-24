use parasolid_core::{FieldValue, RawNode};
use std::collections::BTreeMap;
type R<T> = Result<T, String>;
fn values<'a>(n: &'a RawNode, name: &str) -> R<&'a [FieldValue]> {
    Ok(&n.fields.iter().find(|f| f.definition.name == name).ok_or_else(|| format!("missing color field {name}"))?.values)
}
fn ptr(n: &RawNode, name: &str) -> R<u32> {
    match values(n, name)? {
        [FieldValue::PointerIndex(v)] => Ok(*v),
        _ => Err("invalid color pointer".into()),
    }
}
pub fn read(nodes: &[RawNode]) -> R<BTreeMap<u32, [f64; 3]>> {
    let nodes = nodes.iter().map(|n| (n.index, n)).collect::<BTreeMap<_, _>>();
    let mut result = BTreeMap::new();
    for n in nodes.values().filter(|n| n.node_type == 81) {
        let def = nodes.get(&ptr(n, "definition_ref")?).ok_or("attribute definition")?;
        if def.node_type != 80 {
            return Err("attribute definition type".into());
        }
        let key = nodes.get(&ptr(def, "identifier_ref")?).ok_or("attribute identifier")?;
        if key.node_type != 79 {
            return Err("attribute identifier type".into());
        }
        let text = values(key, "identifier_bytes")?
            .iter()
            .map(|v| match v {
                FieldValue::Character(c) => Ok(*c),
                _ => Err("attribute identifier byte"),
            })
            .collect::<Result<Vec<_>, _>>()?;
        if text != b"SDL/TYSA_COLOUR" && text != b"SDL/TYSA_COLOUR_2" {
            continue;
        }
        let refs = values(n, "value_arrays")?;
        let [FieldValue::PointerIndex(id)] = refs else { return Err("color attribute arity".into()) };
        let rgb = nodes.get(id).ok_or("color values")?;
        if rgb.node_type != 83 {
            return Err("color values type".into());
        }
        let rgb = values(rgb, "reals")?
            .iter()
            .map(|v| match v {
                FieldValue::Double(Some(v)) if v.is_finite() && (0. ..=1.).contains(v) => Ok(*v),
                _ => Err("invalid RGB value"),
            })
            .collect::<Result<Vec<_>, _>>()?;
        if rgb.len() != 3 {
            return Err(format!("RGB values {}", rgb.len()));
        }
        let c = <[f64; 3]>::try_from(rgb.as_slice()).map_err(|_| "invalid RGB arity")?;
        let owner = ptr(n, "owner_ref")?;
        if result.insert(owner, c).is_some_and(|old| old != c) {
            return Err("conflicting source colors".into());
        }
    }
    Ok(result)
}
