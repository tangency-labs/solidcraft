//! Bake the saved shape-key state into the editable cage before modifiers.
//! Coordinates, weights and animation references come from the file's SDNA.
use super::{BTreeMap, BTreeSet, Result, Vec3, View, bad, legacy_array, legacy_array_at};
use crate::blend::dna::Numbers;

const MAX_KEYS: usize = 256;
const MAX_GROUPS: usize = 4096;
const MAX_MASK_VALUES: usize = 8_000_000;

struct Key<'a> {
    data: &'a [u8],
    position: f64,
    interpolation: u64,
    influence: f64,
    relative: usize,
    muted: bool,
    group: String,
}

fn spend(budget: &mut usize, amount: usize) -> Result<()> {
    *budget = budget.checked_sub(amount).ok_or_else(|| bad("shape-key work exceeds the file budget"))?;
    Ok(())
}

fn finite(view: View<'_, '_>, field: &str) -> Result<f64> {
    let value = view.file.numbers.float(view.field(field)?, 0)?;
    if !value.is_finite() {
        return Err(bad(format!("non-finite shape-key {field}")));
    }
    Ok(value)
}

fn keys<'a>(key: View<'_, 'a>, vertices: usize, budget: &mut usize) -> Result<(Vec<Key<'a>>, usize)> {
    if key.has("adt") && key.ptr("adt")? != 0 {
        return Err(bad("animated shape keys require evaluated geometry"));
    }
    if key.uint("elemsize", 4)? != 12 {
        return Err(bad("unsupported shape-key coordinate layout"));
    }
    let count = key.uint("totkey", 4)? as usize;
    if !(1..=MAX_KEYS).contains(&count) {
        return Err(bad("shape-key count is empty or exceeds 256"));
    }
    spend(budget, count * vertices)?;
    let list = key.nested("block")?;
    let mut address = list.ptr("first")?;
    let basis_address = key.ptr("refkey")?;
    let mut basis = None;
    let mut previous = 0;
    let mut seen = BTreeSet::new();
    let mut out = Vec::with_capacity(count);
    for index in 0..count {
        if address == 0 || !seen.insert(address) {
            return Err(bad("invalid shape-key list"));
        }
        let block = key.local_block(address)?;
        if block.count != 1 || key.file.structs.get(block.sdna).is_none_or(|s| s.name != "KeyBlock") {
            return Err(bad("invalid shape-key block layout"));
        }
        let view = key.file.view(block, "KeyBlock")?;
        if view.ptr("prev")? != previous || view.uint("totelem", 4)? != vertices as u64 {
            return Err(bad("shape-key list or vertex count does not match the mesh"));
        }
        let data = view.data(view.ptr("data")?)?;
        if data.len() != vertices * 12 {
            return Err(bad("shape-key coordinate array does not match the mesh"));
        }
        if address == basis_address {
            basis = Some(index);
        }
        out.push(Key {
            data,
            position: finite(view, "pos")?,
            interpolation: view.uint("type", 2)?,
            influence: finite(view, "curval")?,
            relative: view.uint("relative", 2)? as usize,
            muted: view.uint("flag", 2)? & 1 != 0,
            group: view.string("vgroup")?,
        });
        previous = address;
        address = view.ptr("next")?;
    }
    if address != 0 || previous != list.ptr("last")? {
        return Err(bad("shape-key list length does not match its count"));
    }
    Ok((out, basis.ok_or_else(|| bad("shape-key reference is outside the list"))?))
}

fn point(numbers: Numbers, key: &Key<'_>, index: usize) -> Result<Vec3> {
    let offset = index * 12;
    let p = Vec3::new(numbers.float(key.data, offset)?, numbers.float(key.data, offset + 4)?, numbers.float(key.data, offset + 8)?);
    if !p.is_finite() {
        return Err(bad("non-finite shape-key coordinate"));
    }
    Ok(p)
}

fn deform_address(mesh: View<'_, '_>) -> Result<u64> {
    if mesh.has("dvert") {
        let address = mesh.ptr("dvert")?;
        if address != 0 {
            return Ok(address);
        }
    }
    if mesh.has("vdata") {
        let vdata = mesh.nested("vdata")?;
        let count = vdata.uint("totlayer", 4)? as usize;
        if count > 4096 {
            return Err(bad("too many vertex custom-data layers"));
        }
        if count != 0 {
            for layer in legacy_array(vdata, "layers", "CustomDataLayer", count)? {
                if layer.uint("type", 4)? == 2 {
                    return layer.ptr("data");
                }
            }
        }
    }
    Ok(0)
}

fn masks(
    object: View<'_, '_>,
    mesh: View<'_, '_>,
    vertices: usize,
    wanted: BTreeSet<&str>,
    budget: &mut usize,
) -> Result<BTreeMap<String, Vec<f64>>> {
    if wanted.is_empty() {
        return Ok(BTreeMap::new());
    }
    let list = if mesh.has("vertex_group_names") { mesh.nested("vertex_group_names")? } else { object.nested("defbase")? };
    let mut address = list.ptr("first")?;
    let mut seen = BTreeSet::new();
    let mut selected = BTreeMap::new();
    let mut names = BTreeSet::new();
    while address != 0 {
        let index = seen.len();
        if index == MAX_GROUPS || !seen.insert(address) {
            return Err(bad("invalid shape-key vertex-group list"));
        }
        let group = list.view(list.data(address)?, "bDeformGroup")?;
        let name = group.string("name")?;
        if wanted.contains(name.as_str()) {
            if !names.insert(name.clone()) {
                return Err(bad("duplicate shape-key vertex-group name"));
            }
            selected.insert(index, (name, Vec::new()));
        }
        address = group.ptr("next")?;
    }
    // Blender applies the full value when the named group is absent or the mesh
    // has no deform data. With deform data present, unassigned vertices get zero.
    if selected.is_empty() {
        return Ok(BTreeMap::new());
    }
    let deform = deform_address(mesh)?;
    if deform == 0 {
        return Ok(BTreeMap::new());
    }
    if selected.len() > MAX_MASK_VALUES.checked_div(vertices).unwrap_or(usize::MAX) {
        return Err(bad("shape-key masks exceed the allocation budget"));
    }
    spend(budget, selected.len() * vertices)?;
    for (_, values) in selected.values_mut() {
        values.try_reserve_exact(vertices).map_err(|_| bad("shape-key mask allocation failed"))?;
        values.resize(vertices, 0.0);
    }
    for (index, vertex) in legacy_array_at(mesh, deform, "MDeformVert", vertices)?.into_iter().enumerate() {
        let count = vertex.uint("totweight", 4)? as usize;
        if count == 0 {
            continue;
        }
        if count > seen.len() {
            return Err(bad("too many shape-key weights on a vertex"));
        }
        spend(budget, count)?;
        let mut groups = BTreeSet::new();
        for weight in legacy_array(vertex, "dw", "MDeformWeight", count)? {
            let group = weight.uint("def_nr", 4)? as usize;
            if group >= seen.len() || !groups.insert(group) {
                return Err(bad("invalid or duplicate shape-key vertex-group weight"));
            }
            if let Some((_, values)) = selected.get_mut(&group) {
                let value = finite(weight, "weight")?;
                if !(0.0..=1.0).contains(&value) {
                    return Err(bad("shape-key weights must be in 0..1"));
                }
                values[index] = value;
            }
        }
    }
    Ok(selected.into_values().collect())
}

fn interpolate(kind: u64, t: f64) -> Result<[f64; 4]> {
    let u = 1.0 - t;
    let t2 = t * t;
    let t3 = t2 * t;
    match kind {
        0 => Ok([0.0, u, t, 0.0]),
        1 | 3 => {
            // Cubic Hermite basis with centered tangents. Blender's Cardinal
            // mode scales the centered differences by .71, Catmull-Rom by .5.
            let scale = if kind == 1 { 0.71 } else { 0.5 };
            let left = scale * t * u * u;
            let right = -scale * t2 * u;
            Ok([-left, 1.0 - 3.0 * t2 + 2.0 * t3 - right, 3.0 * t2 - 2.0 * t3 + left, right])
        }
        2 => Ok([u * u * u / 6.0, (4.0 - 6.0 * t2 + 3.0 * t3) / 6.0, (1.0 + 3.0 * t + 3.0 * t2 - 3.0 * t3) / 6.0, t3 / 6.0]),
        _ => Err(bad("unsupported absolute shape-key interpolation")),
    }
}

fn absolute_terms(keys: &[Key<'_>], time: f64) -> Result<Vec<(usize, f64)>> {
    if keys.len() == 1 {
        return Ok(vec![(0, 1.0)]);
    }
    if keys.windows(2).any(|pair| pair[0].position >= pair[1].position) {
        return Err(bad("absolute shape-key positions must increase"));
    }
    let last = keys.len() - 1;
    let time = time.clamp(keys[0].position, keys[last].position);
    let right = keys.partition_point(|key| key.position <= time).clamp(1, last);
    let left = right - 1;
    let t = (time - keys[left].position) / (keys[right].position - keys[left].position);
    let a = interpolate(keys[left].interpolation, t)?;
    let b = interpolate(keys[right].interpolation, t)?;
    let indices = [left.saturating_sub(1), left, right, (right + 1).min(last)];
    Ok(indices.into_iter().zip(a.into_iter().zip(b).map(|(a, b)| a * (1.0 - t) + b * t)).collect())
}

pub(super) fn apply(object: View<'_, '_>, mesh: View<'_, '_>, points: &mut [Vec3], budget: &mut usize) -> Result<()> {
    if !mesh.has("key") || mesh.ptr("key")? == 0 {
        return Ok(());
    }
    let block = mesh.file.block(mesh.ptr("key")?)?;
    if block.code != "KE" || block.count != 1 || mesh.file.structs.get(block.sdna).is_none_or(|s| s.name != "Key") {
        return Err(bad("invalid mesh shape-key record"));
    }
    let key = mesh.file.view(block, "Key")?;
    let (keys, basis) = keys(key, points.len(), budget)?;
    let numbers = mesh.file.numbers;
    let mode = key.uint("type", 1)?;
    if mode > 1 {
        return Err(bad("unsupported shape-key mode"));
    }
    let show_only = object.has("shapeflag") && object.uint("shapeflag", 1)? & 1 != 0;
    if mode == 0 && !show_only {
        let terms = absolute_terms(&keys, finite(key, "ctime")? * 0.01)?;
        points.fill(Vec3::ZERO);
        for (key, weight) in terms {
            if weight == 0.0 {
                continue;
            }
            for (i, p) in points.iter_mut().enumerate() {
                *p = solidcore_ops::mesh_edit::deform(*p, point(numbers, &keys[key], i)?, Vec3::ZERO, weight)
                    .ok_or_else(|| bad("non-finite absolute shape-key result"))?;
            }
        }
        return Ok(());
    }

    for (i, p) in points.iter_mut().enumerate() {
        *p = point(numbers, &keys[basis], i)?;
    }
    let active = if show_only {
        let index = object.uint("shapenr", 2)? as usize;
        let index = index.saturating_sub(1);
        let active = keys.get(index).ok_or_else(|| bad("active shape key is outside the list"))?;
        (!active.muted && index != basis).then_some((index, active))
    } else {
        None
    };
    let terms: Vec<_> = if show_only {
        active.into_iter().map(|(_, key)| (key, &keys[basis], 1.0)).collect()
    } else {
        keys.iter()
            .enumerate()
            .filter(|&(index, key)| index != basis && !key.muted && key.influence != 0.0)
            .map(|(_, key)| {
                let reference = keys.get(key.relative).ok_or_else(|| bad("relative shape-key reference is outside the list"))?;
                Ok((key, reference, key.influence))
            })
            .collect::<Result<_>>()?
    };
    let wanted = terms.iter().map(|(key, _, _)| key.group.as_str()).filter(|name| !name.is_empty()).collect();
    let masks = masks(object, mesh, points.len(), wanted, budget)?;
    for (key, reference, influence) in terms {
        let mask = masks.get(&key.group);
        for (i, p) in points.iter_mut().enumerate() {
            let weight = influence * mask.map_or(1.0, |weights| weights[i]);
            if weight != 0.0 {
                *p = solidcore_ops::mesh_edit::deform(*p, point(numbers, key, i)?, point(numbers, reference, i)?, weight)
                    .ok_or_else(|| bad("non-finite relative shape-key result"))?;
            }
        }
    }
    Ok(())
}
