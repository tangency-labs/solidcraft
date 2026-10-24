//! Python-independent parsing core for Parasolid transmit data.

#![doc = include_str!("../README.md")]
#![allow(clippy::module_name_repetitions)]

pub mod brep;
mod comparison;
mod document;
mod error;
mod header;
mod reader;
pub mod schema;
mod text_document;
mod text_reader;
mod user_fields;

pub use comparison::{
    ComparisonDifference, ComparisonOptions, DocumentComparison, compare_xb_documents, compare_xb_xt_documents, compare_xt_documents,
    compare_xt_xb_documents,
};
pub use document::{DocumentLimits, FieldValue, RawField, RawNode, XbDocument, XbTermination, parse_xb, write_xb};
pub use error::{ErrorDetails, ErrorKind, ParseError};
pub use header::{InspectionLimits, XbBinaryFormat, XbHeader, inspect_xb};
pub use reader::BinaryReader;
pub use schema::{
    BuiltinProfileCoverage, BuiltinProfileMetadata, BuiltinProfileRegistry, BuiltinSchemaProfile, BuiltinSchemaProvider, EffectiveSchemaRegistry,
    FieldDefinition, FieldType, InMemorySchemaProvider, ParsedSchemaCatalog, SchemaCatalogLimits, SchemaCoverageReport, SchemaEdit, SchemaKey,
    SchemaLimits, SchemaProvider, SchemaProviderProvenance, SchemaProviderResolution, SchemaResolution, SchemaSource, SchemaTypeLookup,
    TypeDefinition, decode_embedded_schema, parse_schema_catalog,
};
pub use text_document::{XtDocument, XtHeader, XtTermination, inspect_xt, parse_xt};
#[doc(hidden)]
pub use text_document::{XtNodeTypeCounts, scan_xt_node_types};

/// Bounded readers for partial embedded transmit fragments.
pub mod partial;
