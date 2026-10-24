//! Encoding-independent comparison of parsed Parasolid raw documents.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::{
    ErrorDetails, ErrorKind, FieldDefinition, FieldType, FieldValue, ParseError, RawNode, SchemaKey, SchemaResolution, TypeDefinition, XbDocument,
    XtDocument,
};

/// Bounds and tolerances for one normalized document comparison.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComparisonOptions {
    /// Absolute tolerance applied to each decoded floating-point component.
    pub absolute_tolerance: f64,
    /// Relative tolerance applied to each decoded floating-point component.
    pub relative_tolerance: f64,
    /// Maximum retained difference records; total differences are still counted.
    pub max_differences: usize,
}

impl Default for ComparisonOptions {
    fn default() -> Self {
        Self { absolute_tolerance: 1.0e-12, relative_tolerance: 1.0e-12, max_differences: 1_000 }
    }
}

/// One normalized difference, independent of physical byte layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComparisonDifference {
    /// Stable dotted difference code.
    pub code: &'static str,
    /// Schema, topology, or field-value category.
    pub category: &'static str,
    /// Human-readable explanation.
    pub message: String,
    /// Node type, when comparison reached a node pair.
    pub node_type: Option<u16>,
    /// Left source index; indices are reported but not compared as semantic values.
    pub left_node_index: Option<u32>,
    /// Right source index; indices are reported but not compared as semantic values.
    pub right_node_index: Option<u32>,
    /// Effective field name, when applicable.
    pub field_name: Option<String>,
    /// Element position within an array field, when applicable.
    pub value_index: Option<usize>,
    /// Debug representation of the left value, when applicable.
    pub left_value: Option<String>,
    /// Debug representation of the right value, when applicable.
    pub right_value: Option<String>,
}

/// Staged L3 comparison report for two complete raw documents.
// Each flag is an independently useful public gate in the staged report.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentComparison {
    /// True when all normalized schema, topology, and field checks passed.
    pub equivalent: bool,
    /// Whether the internal schema keys are byte-for-byte equal.
    pub schema_key_equal: bool,
    /// Whether all encountered effective definitions are structurally equal.
    pub schema_coverage_equal: bool,
    /// Whether each node type occurs the same number of times.
    pub node_type_counts_equal: bool,
    /// Whether source indices happen to name the same node types; informational only.
    pub node_index_layout_equal: bool,
    /// Whether the normalized pointer graph is equal after index remapping.
    pub topology_equal: bool,
    /// Whether all non-pointer field values are equal within tolerance.
    pub field_values_equal: bool,
    /// Number of left nodes.
    pub left_node_count: usize,
    /// Number of right nodes.
    pub right_node_count: usize,
    /// Number of node pairs visited through root/pointer or residual matching.
    pub compared_node_count: usize,
    /// Total differences, including records omitted by `max_differences`.
    pub difference_count: usize,
    /// Whether retained differences were capped.
    pub differences_truncated: bool,
    /// Retained structured differences.
    pub differences: Vec<ComparisonDifference>,
}

/// Compare two binary documents while ignoring byte ranges and source order.
///
/// # Errors
///
/// Returns `comparison.invalid_option` for invalid tolerances or limits.
pub fn compare_xb_documents(left: &XbDocument, right: &XbDocument, options: ComparisonOptions) -> Result<DocumentComparison, ParseError> {
    compare_views(DocumentView::from_xb(left), DocumentView::from_xb(right), options)
}

/// Compare two text documents while ignoring wrapping, byte ranges, and source order.
///
/// # Errors
///
/// Returns `comparison.invalid_option` for invalid tolerances or limits.
pub fn compare_xt_documents(left: &XtDocument, right: &XtDocument, options: ComparisonOptions) -> Result<DocumentComparison, ParseError> {
    compare_views(DocumentView::from_xt(left), DocumentView::from_xt(right), options)
}

/// Compare a binary document with a text document using normalized raw values.
///
/// # Errors
///
/// Returns `comparison.invalid_option` for invalid tolerances or limits.
pub fn compare_xb_xt_documents(left: &XbDocument, right: &XtDocument, options: ComparisonOptions) -> Result<DocumentComparison, ParseError> {
    compare_views(DocumentView::from_xb(left), DocumentView::from_xt(right), options)
}

/// Compare a text document with a binary document using normalized raw values.
///
/// # Errors
///
/// Returns `comparison.invalid_option` for invalid tolerances or limits.
pub fn compare_xt_xb_documents(left: &XtDocument, right: &XbDocument, options: ComparisonOptions) -> Result<DocumentComparison, ParseError> {
    compare_views(DocumentView::from_xt(left), DocumentView::from_xb(right), options)
}

#[derive(Clone, Copy)]
struct DocumentView<'a> {
    schema_key: &'a SchemaKey,
    schemas: &'a [SchemaResolution],
    nodes: &'a [RawNode],
}

impl<'a> DocumentView<'a> {
    fn from_xb(document: &'a XbDocument) -> Self {
        Self { schema_key: &document.schema_key, schemas: &document.schemas, nodes: &document.nodes }
    }

    fn from_xt(document: &'a XtDocument) -> Self {
        Self { schema_key: &document.schema_key, schemas: &document.schemas, nodes: &document.nodes }
    }
}

// Keeping the four categories separate prevents one mismatch from obscuring
// which validation stage failed in the public report.
#[allow(clippy::struct_excessive_bools)]
struct ComparisonBuilder {
    max_differences: usize,
    differences: Vec<ComparisonDifference>,
    difference_count: usize,
    schema_equal: bool,
    node_counts_equal: bool,
    topology_equal: bool,
    field_values_equal: bool,
}

impl ComparisonBuilder {
    fn new(max_differences: usize) -> Self {
        Self {
            max_differences,
            differences: Vec::new(),
            difference_count: 0,
            schema_equal: true,
            node_counts_equal: true,
            topology_equal: true,
            field_values_equal: true,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn difference(
        &mut self,
        code: &'static str,
        category: &'static str,
        message: impl Into<String>,
        node_type: Option<u16>,
        left_node_index: Option<u32>,
        right_node_index: Option<u32>,
        field_name: Option<&str>,
        value_index: Option<usize>,
        left_value: Option<String>,
        right_value: Option<String>,
    ) {
        self.difference_count = self.difference_count.saturating_add(1);
        match category {
            "schema" => self.schema_equal = false,
            "topology" => self.topology_equal = false,
            "field_value" => self.field_values_equal = false,
            "node_count" => self.node_counts_equal = false,
            _ => {}
        }
        if self.differences.len() < self.max_differences {
            self.differences.push(ComparisonDifference {
                code,
                category,
                message: message.into(),
                node_type,
                left_node_index,
                right_node_index,
                field_name: field_name.map(str::to_owned),
                value_index,
                left_value,
                right_value,
            });
        }
    }
}

#[allow(clippy::too_many_lines)]
fn compare_views(left: DocumentView<'_>, right: DocumentView<'_>, options: ComparisonOptions) -> Result<DocumentComparison, ParseError> {
    validate_options(options)?;
    let mut builder = ComparisonBuilder::new(options.max_differences);
    let schema_key_equal = left.schema_key == right.schema_key;
    compare_schemas(left.schemas, right.schemas, &mut builder);
    let left_counts = node_type_counts(left.nodes);
    let right_counts = node_type_counts(right.nodes);
    if left_counts != right_counts {
        builder.difference(
            "comparison.node_type_count_mismatch",
            "node_count",
            "node type counts differ",
            None,
            None,
            None,
            None,
            None,
            Some(format!("{left_counts:?}")),
            Some(format!("{right_counts:?}")),
        );
    }

    let left_by_index = nodes_by_index(left.nodes);
    let right_by_index = nodes_by_index(right.nodes);
    let node_index_layout_equal = index_layout(&left_by_index) == index_layout(&right_by_index);
    let mut mapping = BTreeMap::new();
    let mut reverse_mapping = BTreeMap::new();
    let mut queue = VecDeque::new();
    let mut processed = BTreeSet::new();
    let mut unmatched_left = BTreeSet::new();

    match (left_by_index.get(&1), right_by_index.get(&1)) {
        (Some(_), Some(_)) => enqueue_pair(1, 1, &mut mapping, &mut reverse_mapping, &mut queue, &mut builder),
        (None, None) if left.nodes.is_empty() && right.nodes.is_empty() => {}
        _ => builder.difference(
            "comparison.root_node_mismatch",
            "topology",
            "one document does not contain the required root node at index 1",
            None,
            left_by_index.get(&1).map(|node| node.index),
            right_by_index.get(&1).map(|node| node.index),
            None,
            None,
            None,
            None,
        ),
    }

    process_queue(&left_by_index, &right_by_index, &mut mapping, &mut reverse_mapping, &mut queue, &mut processed, &mut builder, options);

    loop {
        let Some((&left_index, left_node)) =
            left_by_index.iter().find(|(index, _)| !mapping.contains_key(*index) && !unmatched_left.contains(*index))
        else {
            break;
        };
        let candidates = right_by_index
            .iter()
            .filter(|(index, right_node)| !reverse_mapping.contains_key(index) && local_shape_compatible(left_node, right_node))
            .map(|(index, _)| *index)
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            unmatched_left.insert(left_index);
            builder.difference(
                "comparison.unmatched_node",
                "topology",
                "left node has no structurally compatible right node",
                Some(left_node.node_type),
                Some(left_index),
                None,
                None,
                None,
                None,
                None,
            );
            continue;
        }
        let right_index = candidates.iter().copied().find(|candidate| *candidate == left_index).unwrap_or(candidates[0]);
        enqueue_pair(left_index, right_index, &mut mapping, &mut reverse_mapping, &mut queue, &mut builder);
        process_queue(&left_by_index, &right_by_index, &mut mapping, &mut reverse_mapping, &mut queue, &mut processed, &mut builder, options);
    }

    for (&right_index, right_node) in &right_by_index {
        if !reverse_mapping.contains_key(&right_index) {
            builder.difference(
                "comparison.unmatched_node",
                "topology",
                "right node has no structurally compatible left node",
                Some(right_node.node_type),
                None,
                Some(right_index),
                None,
                None,
                None,
                None,
            );
        }
    }

    let schema_coverage_equal = builder.schema_equal;
    let node_type_counts_equal = builder.node_counts_equal;
    let topology_equal = builder.topology_equal;
    let field_values_equal = builder.field_values_equal;
    let equivalent = schema_coverage_equal && node_type_counts_equal && topology_equal && field_values_equal;
    Ok(DocumentComparison {
        equivalent,
        schema_key_equal,
        schema_coverage_equal,
        node_type_counts_equal,
        node_index_layout_equal,
        topology_equal,
        field_values_equal,
        left_node_count: left.nodes.len(),
        right_node_count: right.nodes.len(),
        compared_node_count: processed.len(),
        difference_count: builder.difference_count,
        differences_truncated: builder.difference_count > builder.differences.len(),
        differences: builder.differences,
    })
}

#[allow(clippy::too_many_arguments)]
fn process_queue(
    left_by_index: &BTreeMap<u32, &RawNode>,
    right_by_index: &BTreeMap<u32, &RawNode>,
    mapping: &mut BTreeMap<u32, u32>,
    reverse_mapping: &mut BTreeMap<u32, u32>,
    queue: &mut VecDeque<(u32, u32)>,
    processed: &mut BTreeSet<(u32, u32)>,
    builder: &mut ComparisonBuilder,
    options: ComparisonOptions,
) {
    while let Some((left_index, right_index)) = queue.pop_front() {
        if !processed.insert((left_index, right_index)) {
            continue;
        }
        let (Some(left), Some(right)) = (left_by_index.get(&left_index).copied(), right_by_index.get(&right_index).copied()) else {
            continue;
        };
        compare_node_pair(left, right, left_by_index, right_by_index, mapping, reverse_mapping, queue, builder, options);
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn compare_node_pair(
    left: &RawNode,
    right: &RawNode,
    left_by_index: &BTreeMap<u32, &RawNode>,
    right_by_index: &BTreeMap<u32, &RawNode>,
    mapping: &mut BTreeMap<u32, u32>,
    reverse_mapping: &mut BTreeMap<u32, u32>,
    queue: &mut VecDeque<(u32, u32)>,
    builder: &mut ComparisonBuilder,
    options: ComparisonOptions,
) {
    if left.node_type != right.node_type {
        builder.difference(
            "comparison.node_type_mismatch",
            "topology",
            "mapped nodes have different types",
            None,
            Some(left.index),
            Some(right.index),
            None,
            None,
            Some(left.node_type.to_string()),
            Some(right.node_type.to_string()),
        );
        return;
    }
    let node_type = Some(left.node_type);
    if left.user_fields != right.user_fields {
        builder.difference(
            "comparison.user_fields_mismatch",
            "field_value",
            "mapped nodes contain different application user-field words",
            node_type,
            Some(left.index),
            Some(right.index),
            None,
            None,
            Some(format!("{:?}", left.user_fields)),
            Some(format!("{:?}", right.user_fields)),
        );
    }
    if !definition_equal(&left.definition, &right.definition) {
        builder.difference(
            "comparison.node_definition_mismatch",
            "schema",
            "mapped nodes use different effective definitions",
            node_type,
            Some(left.index),
            Some(right.index),
            None,
            None,
            None,
            None,
        );
        return;
    }
    if left.variable_length != right.variable_length {
        builder.difference(
            "comparison.variable_length_mismatch",
            "field_value",
            "mapped variable nodes have different lengths",
            node_type,
            Some(left.index),
            Some(right.index),
            None,
            None,
            Some(format!("{:?}", left.variable_length)),
            Some(format!("{:?}", right.variable_length)),
        );
    }
    if left.fields.len() != right.fields.len() {
        builder.difference(
            "comparison.field_count_mismatch",
            "schema",
            "mapped nodes contain different effective field counts",
            node_type,
            Some(left.index),
            Some(right.index),
            None,
            None,
            Some(left.fields.len().to_string()),
            Some(right.fields.len().to_string()),
        );
        return;
    }

    for (left_field, right_field) in left.fields.iter().zip(&right.fields) {
        if left_field.definition != right_field.definition {
            builder.difference(
                "comparison.field_definition_mismatch",
                "schema",
                "mapped fields use different definitions",
                node_type,
                Some(left.index),
                Some(right.index),
                Some(&left_field.definition.name),
                None,
                None,
                None,
            );
            continue;
        }
        if left_field.values.len() != right_field.values.len() {
            builder.difference(
                "comparison.field_value_count_mismatch",
                "field_value",
                "mapped fields contain different value counts",
                node_type,
                Some(left.index),
                Some(right.index),
                Some(&left_field.definition.name),
                None,
                Some(left_field.values.len().to_string()),
                Some(right_field.values.len().to_string()),
            );
            continue;
        }
        for (value_index, (left_value, right_value)) in left_field.values.iter().zip(&right_field.values).enumerate() {
            if left_field.definition.field_type == FieldType::PointerIndex {
                compare_pointer_pair(
                    left_value,
                    right_value,
                    left,
                    right,
                    &left_field.definition,
                    value_index,
                    left_by_index,
                    right_by_index,
                    mapping,
                    reverse_mapping,
                    queue,
                    builder,
                );
            } else if !field_value_equal(left_value, right_value, options) {
                builder.difference(
                    "comparison.field_value_mismatch",
                    "field_value",
                    "mapped non-pointer field values differ",
                    node_type,
                    Some(left.index),
                    Some(right.index),
                    Some(&left_field.definition.name),
                    Some(value_index),
                    Some(format!("{left_value:?}")),
                    Some(format!("{right_value:?}")),
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn compare_pointer_pair(
    left_value: &FieldValue,
    right_value: &FieldValue,
    left_node: &RawNode,
    right_node: &RawNode,
    field: &FieldDefinition,
    value_index: usize,
    left_by_index: &BTreeMap<u32, &RawNode>,
    right_by_index: &BTreeMap<u32, &RawNode>,
    mapping: &mut BTreeMap<u32, u32>,
    reverse_mapping: &mut BTreeMap<u32, u32>,
    queue: &mut VecDeque<(u32, u32)>,
    builder: &mut ComparisonBuilder,
) {
    let (FieldValue::PointerIndex(left_index), FieldValue::PointerIndex(right_index)) = (left_value, right_value) else {
        builder.difference(
            "comparison.pointer_value_type_mismatch",
            "topology",
            "pointer field did not contain pointer values in both documents",
            Some(left_node.node_type),
            Some(left_node.index),
            Some(right_node.index),
            Some(&field.name),
            Some(value_index),
            Some(format!("{left_value:?}")),
            Some(format!("{right_value:?}")),
        );
        return;
    };
    let left_target = (*left_index != 0).then(|| left_by_index.get(left_index).copied()).flatten();
    let right_target = (*right_index != 0).then(|| right_by_index.get(right_index).copied()).flatten();
    match (left_target, right_target) {
        (None, None) => {}
        (Some(_), Some(_)) => enqueue_pair(*left_index, *right_index, mapping, reverse_mapping, queue, builder),
        _ => builder.difference(
            "comparison.pointer_resolution_mismatch",
            "topology",
            "one pointer resolves to a transmitted node while the other is null or unresolved",
            Some(left_node.node_type),
            Some(left_node.index),
            Some(right_node.index),
            Some(&field.name),
            Some(value_index),
            Some(left_index.to_string()),
            Some(right_index.to_string()),
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn enqueue_pair(
    left_index: u32,
    right_index: u32,
    mapping: &mut BTreeMap<u32, u32>,
    reverse_mapping: &mut BTreeMap<u32, u32>,
    queue: &mut VecDeque<(u32, u32)>,
    builder: &mut ComparisonBuilder,
) {
    if mapping.get(&left_index).is_some_and(|existing| *existing != right_index)
        || reverse_mapping.get(&right_index).is_some_and(|existing| *existing != left_index)
    {
        builder.difference(
            "comparison.pointer_mapping_conflict",
            "topology",
            "pointer traversal requires a non-bijective node mapping",
            None,
            Some(left_index),
            Some(right_index),
            None,
            None,
            None,
            None,
        );
        return;
    }
    if mapping.insert(left_index, right_index).is_none() {
        reverse_mapping.insert(right_index, left_index);
        queue.push_back((left_index, right_index));
    }
}

fn compare_schemas(left: &[SchemaResolution], right: &[SchemaResolution], builder: &mut ComparisonBuilder) {
    let left_map = left.iter().map(|item| (item.definition.node_type, &item.definition)).collect::<BTreeMap<_, _>>();
    let right_map = right.iter().map(|item| (item.definition.node_type, &item.definition)).collect::<BTreeMap<_, _>>();
    let types = left_map.keys().chain(right_map.keys()).copied().collect::<BTreeSet<_>>();
    for node_type in types {
        let equal = match (left_map.get(&node_type), right_map.get(&node_type)) {
            (Some(left), Some(right)) => definition_equal(left, right),
            _ => false,
        };
        if !equal {
            builder.difference(
                "comparison.schema_definition_mismatch",
                "schema",
                "encountered effective schema definitions differ",
                Some(node_type),
                None,
                None,
                None,
                None,
                None,
                None,
            );
        }
    }
}

fn definition_equal(left: &TypeDefinition, right: &TypeDefinition) -> bool {
    left.node_type == right.node_type && left.name == right.name && left.variable == right.variable && left.fields == right.fields
}

fn local_shape_compatible(left: &RawNode, right: &RawNode) -> bool {
    left.node_type == right.node_type
        && definition_equal(&left.definition, &right.definition)
        && left.variable_length == right.variable_length
        && left.fields.len() == right.fields.len()
        && left
            .fields
            .iter()
            .zip(&right.fields)
            .all(|(left_field, right_field)| left_field.definition == right_field.definition && left_field.values.len() == right_field.values.len())
}

fn field_value_equal(left: &FieldValue, right: &FieldValue, options: ComparisonOptions) -> bool {
    match (left, right) {
        (FieldValue::UnsignedByte(left), FieldValue::UnsignedByte(right)) | (FieldValue::Character(left), FieldValue::Character(right)) => {
            left == right
        }
        (FieldValue::Logical(left), FieldValue::Logical(right)) => left == right,
        (FieldValue::ShortInteger(left), FieldValue::ShortInteger(right)) => left == right,
        (FieldValue::UnicodeCharacter(left), FieldValue::UnicodeCharacter(right)) => left == right,
        (FieldValue::Integer(left), FieldValue::Integer(right)) | (FieldValue::Tag(left), FieldValue::Tag(right)) => left == right,
        (FieldValue::PointerIndex(left), FieldValue::PointerIndex(right)) => left == right,
        (FieldValue::Double(left), FieldValue::Double(right)) => optional_double_equal(*left, *right, options),
        (FieldValue::Interval(left), FieldValue::Interval(right)) => double_array_equal(left, right, options),
        (FieldValue::Vector(left), FieldValue::Vector(right)) | (FieldValue::IntersectionPoint(left), FieldValue::IntersectionPoint(right)) => {
            double_array_equal(left, right, options)
        }
        (FieldValue::Box3(left), FieldValue::Box3(right)) => double_array_equal(left, right, options),
        _ => false,
    }
}

fn optional_double_equal(left: Option<f64>, right: Option<f64>, options: ComparisonOptions) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => double_equal(left, right, options),
        _ => false,
    }
}

fn double_array_equal<const COUNT: usize>(left: &[Option<f64>; COUNT], right: &[Option<f64>; COUNT], options: ComparisonOptions) -> bool {
    left.iter().zip(right).all(|(left, right)| optional_double_equal(*left, *right, options))
}

fn double_equal(left: f64, right: f64, options: ComparisonOptions) -> bool {
    if left.to_bits() == right.to_bits() || (left.is_nan() && right.is_nan()) {
        return true;
    }
    if !left.is_finite() || !right.is_finite() {
        return false;
    }
    let difference = (left - right).abs();
    difference <= options.absolute_tolerance + options.relative_tolerance * left.abs().max(right.abs())
}

fn node_type_counts(nodes: &[RawNode]) -> BTreeMap<u16, usize> {
    let mut counts = BTreeMap::new();
    for node in nodes {
        *counts.entry(node.node_type).or_default() += 1;
    }
    counts
}

fn nodes_by_index(nodes: &[RawNode]) -> BTreeMap<u32, &RawNode> {
    nodes.iter().map(|node| (node.index, node)).collect()
}

fn index_layout(nodes: &BTreeMap<u32, &RawNode>) -> Vec<(u32, u16)> {
    nodes.iter().map(|(index, node)| (*index, node.node_type)).collect()
}

fn validate_options(options: ComparisonOptions) -> Result<(), ParseError> {
    for (name, value) in [("absolute_tolerance", options.absolute_tolerance), ("relative_tolerance", options.relative_tolerance)] {
        if !value.is_finite() || value < 0.0 {
            return Err(ParseError::new(
                ErrorKind::InvalidComparisonOption,
                0,
                format!("{name} must be finite and non-negative"),
                ErrorDetails::InvalidText { field: name, value: value.to_string() },
            ));
        }
    }
    if options.max_differences == 0 {
        return Err(ParseError::new(
            ErrorKind::InvalidComparisonOption,
            0,
            "max_differences must be positive",
            ErrorDetails::InvalidLength { field: "max_differences", value: 0 },
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::SchemaSource;

    fn node(index: u32, target: u32, value: f64) -> RawNode {
        let fields = vec![
            FieldDefinition { name: "next".to_owned(), field_type: FieldType::PointerIndex, pointer_class: 12, element_count: 0, transmitted: true },
            FieldDefinition { name: "value".to_owned(), field_type: FieldType::Double, pointer_class: 0, element_count: 0, transmitted: true },
        ];
        let definition = TypeDefinition::from_fields(12, "BODY", "Body", fields.clone(), SchemaSource::Base);
        RawNode {
            node_type: 12,
            index,
            user_fields: Vec::new(),
            variable_length: None,
            definition,
            first_schema: None,
            fields: vec![
                crate::RawField { definition: fields[0].clone(), values: vec![FieldValue::PointerIndex(target)], byte_range: 0..0 },
                crate::RawField { definition: fields[1].clone(), values: vec![FieldValue::Double(Some(value))], byte_range: 0..0 },
            ],
            byte_range: 0..0,
        }
    }

    #[test]
    fn remaps_pointer_indices_and_applies_float_tolerance() -> Result<(), ParseError> {
        let left_nodes = vec![node(1, 2, 1000.0), node(2, 0, 1.0)];
        let right_nodes = vec![node(1, 9, 1_000.000_000_000_000_1), node(9, 0, 1.0)];
        let left_schema = SchemaKey::parse("SCH_3000000_30000")?;
        let right_schema = left_schema.clone();
        let left = DocumentView { schema_key: &left_schema, schemas: &[], nodes: &left_nodes };
        let right = DocumentView { schema_key: &right_schema, schemas: &[], nodes: &right_nodes };

        let report = compare_views(left, right, ComparisonOptions::default())?;

        assert!(report.equivalent);
        assert!(!report.node_index_layout_equal);
        assert_eq!(report.compared_node_count, 2);
        assert!(report.differences.is_empty());
        Ok(())
    }

    #[test]
    fn reports_topology_and_field_differences_separately() -> Result<(), ParseError> {
        let left_nodes = vec![node(1, 2, 10.0), node(2, 0, 1.0)];
        let right_nodes = vec![node(1, 0, 11.0), node(2, 0, 1.0)];
        let schema = SchemaKey::parse("SCH_3000000_30000")?;
        let left = DocumentView { schema_key: &schema, schemas: &[], nodes: &left_nodes };
        let right = DocumentView { schema_key: &schema, schemas: &[], nodes: &right_nodes };

        let report = compare_views(left, right, ComparisonOptions::default())?;

        assert!(!report.equivalent);
        assert!(!report.topology_equal);
        assert!(!report.field_values_equal);
        assert!(report.differences.iter().any(|difference| difference.category == "topology"));
        assert!(report.differences.iter().any(|difference| difference.category == "field_value"));
        Ok(())
    }
}
