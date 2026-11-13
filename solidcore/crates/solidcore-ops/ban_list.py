"""The disallowed-methods list of the layers that compute no geometry (docs/layers.md §4): the
float maths; every public method of the kernel's vector, transform, matrix, placement and extent
impl blocks, less the ones of that type that only build, copy or read fields (KEEP, per type),
and the methods of the crate's public traits implemented for them (`impl Trait for Ty`);
and the kernel's free functions that compute geometry (the float maths under another name, the
NURBS geometry helpers, the certified vector arithmetic), less the comparisons (kept per module).
Prints the whole `disallowed-methods = [...]` array.
usage: python3 ban_list.py <solidcore crate src dir> <reason> [--floats-only]
(--floats-only for solidcore-record, which does not depend on the kernel.)"""
import re, sys
src, reason = sys.argv[1], sys.argv[2]
floats_only = "--floats-only" in sys.argv[3:]
FLOATS = ["sqrt", "cbrt", "hypot", "sin", "cos", "tan", "sin_cos", "asin", "acos", "atan", "atan2",
          "sinh", "cosh", "tanh", "asinh", "acosh", "atanh", "powi", "powf", "exp", "exp2", "exp_m1",
          "ln", "ln_1p", "log", "log2", "log10", "mul_add", "recip", "to_radians", "to_degrees"]
# Per type: (file, type, path) and the methods of that type kept. A name kept on one type opens
# nothing on another. Kept: constructors from given values, field reads and writes, and the
# kernel's certified queries on a transform (`is_regular`, `row_lengths`, `of_placement`), and the
# kernel's maps built from an application's numbers and their composition (`rotation`,
# `uniform_scaling`, `reflection`, `concat`, `inverse`), which ops calls rather than computes.
TYPES = [
    ("geom/vector3d.rs", "Vec3", "solidcore::Vec3",
     {"new", "from_array", "to_array", "set_uninitialized", "is_initialized", "is_undef",
      "component", "component_mut"}),
    ("geom/vector2d.rs", "Vec2", "solidcore::Vec2",
     {"new", "from_array", "to_array", "from_xy", "swap_xy", "set_uninitialized", "is_initialized",
      "component", "component_mut"}),
    ("math.rs", "Transform", "solidcore::Transform",
     {"translation", "x_axis", "y_axis", "z_axis", "translation_part", "is_regular", "row_lengths",
      "of_placement", "rotation", "uniform_scaling", "reflection", "concat",
      "inverse"}),
    ("geom/transform.rs", "Transform", "solidcore::geom::transform::Transform",
     {"new", "matrix", "type_code", "type_string", "class_string", "is_kind_of", "read_from_db"}),
    ("geom/matrix.rs", "Matrix", "solidcore::geom::matrix::Matrix",
     {"new", "try_new", "from_row_major", "num_rows", "num_columns", "as_slice", "at", "set_at",
      "row", "row_mut", "set_size", "clear"}),
    # `new` makes its axes canonical (unit, orthogonal) and `z_axis` is a cross product: both
    # computations. `from_stored` takes the axes as given.
    ("geom/axis2_placement.rs", "Axis2Placement", "solidcore::geom::axis2_placement::Axis2Placement",
     {"init", "from_stored", "origin", "x_axis", "y_axis", "canonical", "set_origin"}),
    ("geom/extent3d.rs", "Extent3d", "solidcore::geom::extent3d::Extent3d",
     {"new", "from_bounds", "init", "is_defined", "is_init", "set_unbounded", "min", "max",
      "u_min", "u_max", "v_min", "v_max", "w_min", "w_max", "u_interval", "v_interval",
      "w_interval", "set_u_interval", "set_v_interval", "set_min_max", "set_min_max_coords",
      "set_u_min", "set_u_max", "set_v_min", "set_v_max", "boundary_types", "is_bounded"}),
    ("geom/extent2d.rs", "Extent2d", "solidcore::geom::extent2d::Extent2d",
     {"new", "from_bounds", "init", "is_init", "set_min_max", "set_min_max_coords",
      "set_u_interval", "set_v_interval", "set_u_min", "set_u_max", "set_v_min", "set_v_max",
      "min", "max", "u_min", "u_max", "v_min", "v_max", "u_interval", "v_interval",
      "boundary_types", "is_bounded"}),
    ("geom/extent1d.rs", "Extent1d", "solidcore::geom::extent1d::Extent1d",
     {"new", "from_min_max", "from_raw", "init", "is_defined", "is_init", "set_unbounded", "min",
      "max", "set_min_max", "set_min", "set_max", "boundary_type", "is_bounded"}),
    ("nurbs/math.rs", "CPoint", "solidcore::nurbs::math::CPoint",
     {"new", "to_array", "from_point", "weighted", "drop_w"}),
    ("nurbs/math.rs", "RealMatrix", "solidcore::nurbs::math::RealMatrix",
     {"null", "new", "from_rows", "define", "n", "m", "max_index", "matrix_type", "bandwidth",
      "storage_shape", "is_null", "get", "set", "check_storage", "ensure"}),
]
# The crate's public traits implemented for these types: every method of the trait, by its trait
# path (a trait method is called through the trait, so `Ty::method` would not catch it), less the
# conversions kept.
TRAIT_KEEP = {"FitPoint": {"to_cpoint"}}
# Free functions, per module: every public one, less the comparisons kept.
FREE = [
    ("geom/vector3d.rs", "solidcore::geom::vector3d", set()),
    ("math.rs", "solidcore::math",
     {"is_infinite_param", "max2", "min2", "max3", "min3", "max4", "min4"}),
    ("nurbs/geom.rs", "solidcore::nurbs::geom", set()),
    ("geom/certified.rs", "solidcore::geom::certified", {"above"}),
    ("geom/geom_utility/p1.rs", "solidcore::geom::geom_utility::p1", set()),
    ("geom/polynomial.rs", "solidcore::geom::polynomial", set()),
    ("nurbs/math.rs", "solidcore::nurbs::math", {"gamma"}),
]


def block_end(s, i):
    depth = 1
    while depth:
        c = s[i]
        depth += (c == '{') - (c == '}')
        i += 1
    return i


def methods(path, ty):
    s = open(path).read()
    out = []
    for m in re.finditer(r'^impl ' + ty + r' \{', s, re.M):
        out += re.findall(r'^    pub (?:const )?fn (\w+)', s[m.end():block_end(s, m.end())], re.M)
    return out


def trait_impls(src, types):
    """The crate's public traits implemented for `types` (`impl Trait for Ty` blocks anywhere in
    the crate): (trait path, its methods)."""
    import os
    names = set()
    defs = {}
    for root, _, files in os.walk(src):
        for f in files:
            if not f.endswith(".rs"):
                continue
            path = os.path.join(root, f)
            s = open(path).read()
            for m in re.finditer(r'^impl(?:<[^>]*>)? ([A-Za-z_][\w:]*)(?:<[^>]*>)? for (\w+)\b', s, re.M):
                if m.group(2) in types:
                    names.add(m.group(1).split("::")[-1])
            for m in re.finditer(r'^pub trait (\w+)[^{]*\{', s, re.M):
                body = s[m.end():block_end(s, m.end())]
                module = os.path.relpath(path, src)[:-3].replace(os.sep, "::")
                module = module[:-len("::mod")] if module.endswith("::mod") else module
                defs[m.group(1)] = ("solidcore::" + module + "::" + m.group(1),
                                    re.findall(r'^    fn (\w+)', body, re.M))
    return [defs[n] for n in sorted(names) if n in defs]


def free_functions(path):
    # the module's own functions, not its tests'
    s = open(path).read().split("#[cfg(test)]")[0]
    return re.findall(r'^pub (?:const )?fn (\w+)', s, re.M)


rows = []
for t in ("f64", "f32"):
    rows += [f'    {{ path = "{t}::{f}", reason = "{reason}" }},' for f in FLOATS]
if not floats_only:
    for file, ty, name, keep in TYPES:
        found = set(methods(src + "/" + file, ty))
        missing = keep - found
        assert not missing, f"{ty}: kept methods not found: {sorted(missing)}"
        for f in sorted(found - keep):
            rows.append(f'    {{ path = "{name}::{f}", reason = "{reason}" }},')
    for trait, fns in trait_impls(src, {ty for _, ty, _, _ in TYPES}):
        keep = TRAIT_KEEP.get(trait.split("::")[-1], set())
        for f in sorted(set(fns) - keep):
            rows.append(f'    {{ path = "{trait}::{f}", reason = "{reason}" }},')
    for file, module, keep in FREE:
        found = set(free_functions(src + "/" + file))
        missing = keep - found
        assert not missing, f"{module}: kept functions not found: {sorted(missing)}"
        for f in sorted(found - keep):
            rows.append(f'    {{ path = "{module}::{f}", reason = "{reason}" }},')
print("disallowed-methods = [\n" + "\n".join(rows) + "\n]")
