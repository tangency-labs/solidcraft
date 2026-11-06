//! PostgreSQL integration tests for the SolidBOM store.
//!
//! Each test runs in its own schema on `SOLIDBOM_TEST_DATABASE_URL` and drops it afterwards. The
//! tests never reset the database they connect to.
//!
//! `#[tokio::test]` is not recognised as a test function by Clippy's test allowance, so the file
//! allows the panicking helpers it needs. The library and binary still deny them.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::str::FromStr;

use rust_decimal::Decimal;
use sha2::{Digest, Sha256};
use solidbom_contract::{
    AnalysisKey, AnalysisManifest, CycleBasis, DefinitionRef, DraftProject, GeometryBinding, Measurement, MeasurementSource, OccurrenceRef,
    OperationInputs, PartInputs, ReviewState, SCHEMA_VERSION, SaveDraft, SourceRef, Sourcing, Unit,
};
use solidbom_server::{Store, StoreError};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use uuid::Uuid;

const FIXTURE_SOURCE: &[u8] =
    b"ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('SolidBOM test fixture'),'2;1');\nENDSEC;\nDATA;\nENDSEC;\nEND-ISO-10303-21;\n";
const IDENTITY: [[f64; 4]; 4] = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

fn d(text: &str) -> Decimal {
    Decimal::from_str(text).expect("a decimal in the fixture")
}

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn material_id() -> Uuid {
    Uuid::from_u128(1)
}

fn rate_id() -> Uuid {
    Uuid::from_u128(0x101)
}

fn key() -> AnalysisKey {
    AnalysisKey { source_sha256: digest(FIXTURE_SOURCE), engine_build: "0.40.0-rc.2".into(), extractor_version: "1".into() }
}

fn measurement(value: Option<&str>) -> Measurement {
    Measurement {
        value: value.map(d),
        unit: Unit::MillimetreCubed,
        source: MeasurementSource::Extracted,
        review: ReviewState::Confirmed,
        reason: None,
        analysis_key: key(),
    }
}

fn part(part_version_id: Uuid) -> PartInputs {
    PartInputs {
        part_version_id,
        classification: "clip".into(),
        classification_confirmed: true,
        sourcing: Sourcing::Manufactured,
        volume_mm3: measurement(Some("2400")),
        material_version_id: Some(material_id()),
        allowance_rate: Some(d("0.02")),
        operation: Some(OperationInputs {
            cycle_seconds: Some(d("30")),
            cycle_basis: CycleBasis::PerPiece,
            cavities: Some(1),
            production_rate_version_id: Some(rate_id()),
            setup_hours: Some(d("0.5")),
            setup_rate_version_id: Some(rate_id()),
        }),
    }
}

fn one_definition(key: AnalysisKey) -> AnalysisManifest {
    let definition_id = Uuid::from_u128(0xdef1);
    let occurrences: Vec<OccurrenceRef> = (0u64..3)
        .map(|index| OccurrenceRef {
            occurrence_id: Uuid::from_u128(0x0cc0 + u128::from(index)),
            definition_id,
            parent_occurrence_id: None,
            source_path: vec![index],
            transform: IDENTITY,
        })
        .collect();
    let bindings: Vec<GeometryBinding> = occurrences
        .iter()
        .map(|occurrence| GeometryBinding {
            source_path: occurrence.source_path.clone(),
            source_body: 0,
            definition_id,
            occurrence_id: occurrence.occurrence_id,
        })
        .collect();
    AnalysisManifest {
        schema_version: SCHEMA_VERSION,
        key: key.clone(),
        definitions: vec![DefinitionRef {
            definition_id,
            source_key: key,
            customer_part_number: Some("PL-100".into()),
            customer_revision: Some("B".into()),
        }],
        occurrences,
        geometry_bindings: bindings,
        issues: Vec::new(),
    }
}

fn fixture_draft(version: u64, source: &SourceRef) -> DraftProject {
    let key = key();
    let analysis = one_definition(key);
    DraftProject {
        project_id: Uuid::from_u128(0x9001),
        product_revision_id: Uuid::from_u128(0x9002),
        version,
        source: source.clone(),
        analysis,
        parts: vec![part(Uuid::from_u128(0xaa01))],
        forecast_year: 2027,
        forecast_quantity: 1000,
        currency: "CNY".into(),
    }
}

fn two_revision_draft(version: u64, source: &SourceRef) -> DraftProject {
    let key = key();
    let mut draft = fixture_draft(version, source);
    let definition_b = draft.analysis.definitions[0].clone();
    let mut definition_c = definition_b.clone();
    definition_c.definition_id = Uuid::from_u128(0xdef2);
    definition_c.customer_revision = Some("C".into());
    let occurrence_c = OccurrenceRef {
        occurrence_id: Uuid::from_u128(0x0cc9),
        definition_id: definition_c.definition_id,
        parent_occurrence_id: None,
        source_path: vec![3],
        transform: IDENTITY,
    };
    draft.analysis.definitions.push(definition_c);
    draft.analysis.geometry_bindings.push(GeometryBinding {
        source_path: occurrence_c.source_path.clone(),
        source_body: 0,
        definition_id: occurrence_c.definition_id,
        occurrence_id: occurrence_c.occurrence_id,
    });
    draft.analysis.occurrences.push(occurrence_c);
    let mut part_c = part(Uuid::from_u128(0xaa02));
    part_c.volume_mm3.analysis_key = key;
    draft.parts.push(part_c);
    draft
}

struct TestDb {
    store: Store,
    options: PgConnectOptions,
    schema: String,
    url: String,
}

impl TestDb {
    async fn new() -> Self {
        let url = std::env::var("SOLIDBOM_TEST_DATABASE_URL").expect("set SOLIDBOM_TEST_DATABASE_URL to a disposable test database");
        let schema = format!("t_{}", Uuid::new_v4().simple());
        let admin = PgPoolOptions::new().max_connections(1).connect(&url).await.expect("connect the test database");
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.expect("create the test schema");
        let options = PgConnectOptions::from_str(&url).expect("parse the test database URL").options([("search_path", schema.as_str())]);
        let store = Store::connect_with_options(options.clone()).await.expect("migrate the test schema");
        Self { store, options, schema, url }
    }

    async fn reconnect(&self) -> Store {
        Store::connect_with_options(self.options.clone()).await.expect("reconnect the test schema")
    }

    async fn cleanup(&self) {
        if let Ok(admin) = PgPoolOptions::new().max_connections(1).connect(&self.url).await {
            let _ = sqlx::query(&format!("DROP SCHEMA {} CASCADE", self.schema)).execute(&admin).await;
        }
    }
}

async fn upload(store: &Store) -> SourceRef {
    store.put_source(&digest(FIXTURE_SOURCE), "three_boxes.step", FIXTURE_SOURCE).await.expect("store the source")
}

async fn create(store: &Store, source: &SourceRef) -> SaveDraft {
    let save = SaveDraft { request_id: "create-1".into(), expected_version: 0, draft: fixture_draft(0, source) };
    let receipt = store.create_draft(&save).await.expect("create the project");
    assert_eq!(receipt.version, 1);
    save
}

#[tokio::test]
async fn a_saved_project_survives_a_reconnect() {
    let db = TestDb::new().await;
    let source = upload(&db.store).await;
    let create = create(&db.store, &source).await;
    let save = SaveDraft { request_id: "save-1".into(), expected_version: 1, draft: fixture_draft(1, &source) };
    let receipt = db.store.save_draft(&save).await.expect("save version 2");
    assert_eq!(receipt.version, 2);

    let store = db.reconnect().await;
    let draft = store.load_project(create.draft.project_id).await.expect("load the project");
    assert_eq!(draft.version, 2);
    assert_eq!(draft.parts, save.draft.parts);
    assert_eq!(draft.source.sha256, digest(FIXTURE_SOURCE));
    assert_eq!(draft.analysis.occurrences.len(), 3);
    assert_eq!(draft.analysis.geometry_bindings.len(), 3);
    db.cleanup().await;
}

#[tokio::test]
async fn a_stale_version_is_refused_and_changes_nothing() {
    let db = TestDb::new().await;
    let source = upload(&db.store).await;
    let create = create(&db.store, &source).await;
    let save = SaveDraft { request_id: "save-1".into(), expected_version: 1, draft: fixture_draft(1, &source) };
    db.store.save_draft(&save).await.expect("save version 2");

    let stale = SaveDraft { request_id: "stale-1".into(), expected_version: 1, draft: fixture_draft(1, &source) };
    let error = db.store.save_draft(&stale).await.expect_err("a stale save is refused");
    assert!(matches!(error, StoreError::Conflict(_)), "{error}");
    assert_eq!(db.store.version_count(create.draft.project_id).await.expect("count"), 2);
    assert_eq!(db.store.load_project(create.draft.project_id).await.expect("load").version, 2);
    db.cleanup().await;
}

#[tokio::test]
async fn an_unknown_source_is_refused_and_leaves_no_project() {
    let db = TestDb::new().await;
    let mut draft = fixture_draft(
        0,
        &SourceRef {
            source_id: Uuid::new_v4(),
            sha256: digest(FIXTURE_SOURCE),
            byte_len: FIXTURE_SOURCE.len() as u64,
            original_name: "three_boxes.step".into(),
        },
    );
    draft.project_id = Uuid::new_v4();
    let save = SaveDraft { request_id: "create-1".into(), expected_version: 0, draft };
    let error = db.store.create_draft(&save).await.expect_err("the source was never uploaded");
    assert!(matches!(error, StoreError::Invalid(_)), "{error}");
    let projects: i64 = sqlx::query_scalar("SELECT count(*) FROM projects").fetch_one(db.store.pool()).await.expect("count projects");
    let versions: i64 = sqlx::query_scalar("SELECT count(*) FROM project_versions").fetch_one(db.store.pool()).await.expect("count versions");
    let requests: i64 = sqlx::query_scalar("SELECT count(*) FROM requests").fetch_one(db.store.pool()).await.expect("count requests");
    assert_eq!((projects, versions, requests), (0, 0, 0));
    db.cleanup().await;
}

#[tokio::test]
async fn the_same_part_number_with_two_revisions_stays_separate() {
    let db = TestDb::new().await;
    let source = upload(&db.store).await;
    let save = SaveDraft { request_id: "create-1".into(), expected_version: 0, draft: two_revision_draft(0, &source) };
    let receipt = db.store.create_draft(&save).await.expect("create two revisions");
    let draft = db.store.load_project(receipt.project_id).await.expect("load");
    assert_eq!(draft.analysis.definitions.len(), 2);
    assert_eq!(draft.parts.len(), 2);
    let revisions: Vec<&str> = draft.analysis.definitions.iter().filter_map(|definition| definition.customer_revision.as_deref()).collect();
    assert_eq!(revisions, ["B", "C"]);
    assert!(draft.analysis.definitions.iter().all(|definition| definition.customer_part_number.as_deref() == Some("PL-100")));
    db.cleanup().await;
}

#[tokio::test]
async fn a_repeated_request_returns_its_receipt_and_a_changed_payload_conflicts() {
    let db = TestDb::new().await;
    let source = upload(&db.store).await;
    let create = create(&db.store, &source).await;
    let save = SaveDraft { request_id: "save-1".into(), expected_version: 1, draft: fixture_draft(1, &source) };
    let first = db.store.save_draft(&save).await.expect("save version 2");
    let retried = db.store.save_draft(&save).await.expect("the lost reply is retried");
    assert_eq!(first, retried);
    assert_eq!(db.store.version_count(create.draft.project_id).await.expect("count"), 2);

    let mut changed = save.clone();
    changed.draft.parts[0].operation.as_mut().expect("operation").cycle_seconds = Some(d("60"));
    let error = db.store.save_draft(&changed).await.expect_err("a changed payload under the same ID conflicts");
    assert!(matches!(error, StoreError::Conflict(_)), "{error}");
    assert_eq!(db.store.version_count(create.draft.project_id).await.expect("count"), 2);
    db.cleanup().await;
}

#[tokio::test]
async fn the_same_source_bytes_are_stored_once() {
    let db = TestDb::new().await;
    let first = upload(&db.store).await;
    let second = upload(&db.store).await;
    assert_eq!(first.source_id, second.source_id);
    let (source, bytes) = db.store.get_source(first.source_id).await.expect("read the source back");
    assert_eq!(source.original_name, "three_boxes.step");
    assert_eq!(bytes, FIXTURE_SOURCE);
    assert_eq!(source.sha256, digest(FIXTURE_SOURCE));
    let mismatched = db.store.put_source(&"0".repeat(64), "other.step", FIXTURE_SOURCE).await;
    assert!(matches!(mismatched, Err(StoreError::Invalid(_))));
    let missing = db.store.get_source(Uuid::new_v4()).await;
    assert!(matches!(missing, Err(StoreError::NotFound)));
    db.cleanup().await;
}

#[tokio::test]
async fn reference_data_is_seeded_with_the_explicit_test_values() {
    let db = TestDb::new().await;
    let data = db.store.reference_data().await.expect("reference data");
    data.validate().expect("the seed is valid");
    let material = data.materials.iter().find(|material| material.density_g_cm3 == d("2.7")).expect("the 2.7 g/cm3 fixture");
    assert_eq!(material.price_per_kg, d("20"));
    assert!(data.rates.iter().any(|rate| rate.rate_per_hour == d("120")));
    db.cleanup().await;
}

#[tokio::test]
async fn a_failed_save_keeps_every_earlier_version() {
    let db = TestDb::new().await;
    let source = upload(&db.store).await;
    let create = create(&db.store, &source).await;
    let save = SaveDraft { request_id: "save-1".into(), expected_version: 1, draft: fixture_draft(1, &source) };
    db.store.save_draft(&save).await.expect("save version 2");

    let mut bad = SaveDraft { request_id: "save-bad".into(), expected_version: 2, draft: fixture_draft(2, &source) };
    bad.draft.source.sha256 = "b".repeat(64);
    bad.draft.analysis.key.source_sha256 = "b".repeat(64);
    for definition in &mut bad.draft.analysis.definitions {
        definition.source_key.source_sha256 = "b".repeat(64);
    }
    bad.draft.parts[0].volume_mm3.analysis_key.source_sha256 = "b".repeat(64);
    bad.draft.version = 2;
    let error = db.store.save_draft(&bad).await.expect_err("the source reference is wrong");
    assert!(matches!(error, StoreError::Invalid(_)), "{error}");
    assert_eq!(db.store.version_count(create.draft.project_id).await.expect("count"), 2);
    let rows: i64 =
        sqlx::query_scalar("SELECT count(*) FROM project_versions WHERE version = 3").fetch_one(db.store.pool()).await.expect("count version 3");
    assert_eq!(rows, 0);
    db.cleanup().await;
}
