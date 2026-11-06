//! PostgreSQL storage for the SolidBOM prototype.
//!
//! Sources are stored once by SHA-256. Project drafts are append-only versions: a save locks the
//! project row, checks the request ID before the expected version, validates the source, inserts
//! the new immutable version and advances the current version in one transaction. A lost reply
//! answered by an explicit retry returns its recorded receipt; a reused request ID with a
//! different payload is a conflict.

use std::str::FromStr;

use rust_decimal::Decimal;
use sha2::{Digest, Sha256};
use solidbom_contract::{
    CalculationSummary, DraftProject, MaterialVersion, ProjectView, RateVersion, ReferenceData, SaveDraft, SaveReceipt, SourceRef,
};
use sqlx::Row;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPool, PgPoolOptions, PgRow};
use uuid::Uuid;

/// What can go wrong in the store. The API maps these; database detail never reaches a client.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("not found")]
    NotFound,
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    Invalid(String),
    #[error("a stored value could not be read: {0}")]
    Codec(String),
    #[error("database error")]
    Database(#[from] sqlx::Error),
    #[error("migrations failed: {0}")]
    Migration(String),
}

impl StoreError {
    fn codec(error: impl std::fmt::Display) -> Self {
        Self::Codec(error.to_string())
    }
}

type Result<T> = std::result::Result<T, StoreError>;

/// A handle on the development database.
#[derive(Clone)]
pub struct Store {
    pool: PgPool,
}

impl Store {
    /// Connect to a database and apply the migrations.
    pub async fn connect(url: &str) -> Result<Self> {
        let options = PgConnectOptions::from_str(url).map_err(|e| StoreError::Invalid(format!("database URL: {e}")))?;
        Self::connect_with_options(options).await
    }

    /// Connect with explicit options (tests use a private schema in the search path).
    pub async fn connect_with_options(options: PgConnectOptions) -> Result<Self> {
        let pool = PgPoolOptions::new().max_connections(8).connect_with(options).await?;
        let store = Self { pool };
        store.migrate().await?;
        Ok(store)
    }

    /// Apply the embedded migrations. Idempotent: SQLx records them.
    pub async fn migrate(&self) -> Result<()> {
        sqlx::migrate!("./migrations").run(&self.pool).await.map_err(|e| StoreError::Migration(e.to_string()))
    }

    /// The connection pool, for the API layer and integration tests.
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Store the original bytes once. The digest must match the bytes; a repeat is idempotent.
    pub async fn put_source(&self, expected_sha256: &str, original_name: &str, bytes: &[u8]) -> Result<SourceRef> {
        let sha256 = hex::encode(Sha256::digest(bytes));
        if sha256 != expected_sha256 {
            return Err(StoreError::Invalid("the uploaded bytes do not match their SHA-256 digest".into()));
        }
        if bytes.is_empty() {
            return Err(StoreError::Invalid("an empty file is not a source".into()));
        }
        if original_name.trim().is_empty() {
            return Err(StoreError::Invalid("a source needs the name it was imported as".into()));
        }
        if let Some(existing) = self.source_by_sha256(&sha256).await? {
            return Ok(existing);
        }
        let source = SourceRef {
            source_id: Uuid::new_v4(),
            sha256,
            byte_len: u64::try_from(bytes.len()).map_err(|_| StoreError::Invalid("the source is too large".into()))?,
            original_name: original_name.to_string(),
        };
        let inserted = sqlx::query("INSERT INTO sources (source_id, sha256, byte_len, original_name, bytes) VALUES ($1, $2, $3, $4, $5)")
            .bind(source.source_id)
            .bind(&source.sha256)
            .bind(i64::try_from(source.byte_len).map_err(|_| StoreError::Invalid("the source is too large".into()))?)
            .bind(&source.original_name)
            .bind(bytes)
            .execute(&self.pool)
            .await;
        match inserted {
            Ok(_) => Ok(source),
            // A concurrent upload of the same bytes won: return the stored row.
            Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
                self.source_by_sha256(&source.sha256).await?.ok_or(StoreError::NotFound)
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Read the original bytes and verify they still match the stored digest.
    pub async fn get_source(&self, source_id: Uuid) -> Result<(SourceRef, Vec<u8>)> {
        let row = sqlx::query("SELECT source_id, sha256, byte_len, original_name, bytes FROM sources WHERE source_id = $1")
            .bind(source_id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or(StoreError::NotFound)?;
        let source = row_to_source(&row)?;
        let bytes: Vec<u8> = row.try_get("bytes")?;
        if hex::encode(Sha256::digest(&bytes)) != source.sha256 {
            return Err(StoreError::Codec("the stored source no longer matches its digest".into()));
        }
        Ok((source, bytes))
    }

    /// The seeded material and rate versions.
    pub async fn reference_data(&self) -> Result<ReferenceData> {
        let material_rows = sqlx::query(
            "SELECT material_version_id, name, density_g_cm3::text AS density, price_per_kg::text AS price, currency \
             FROM material_versions ORDER BY name, material_version_id",
        )
        .fetch_all(&self.pool)
        .await?;
        let mut materials = Vec::with_capacity(material_rows.len());
        for row in &material_rows {
            materials.push(MaterialVersion {
                material_version_id: row.try_get("material_version_id")?,
                name: row.try_get("name")?,
                density_g_cm3: parse_decimal(row.try_get("density")?, "material density")?,
                price_per_kg: parse_decimal(row.try_get("price")?, "material price")?,
                currency: row.try_get("currency")?,
            });
        }
        let rate_rows =
            sqlx::query("SELECT rate_version_id, name, rate_per_hour::text AS rate, currency FROM rate_versions ORDER BY name, rate_version_id")
                .fetch_all(&self.pool)
                .await?;
        let mut rates = Vec::with_capacity(rate_rows.len());
        for row in &rate_rows {
            rates.push(RateVersion {
                rate_version_id: row.try_get("rate_version_id")?,
                name: row.try_get("name")?,
                rate_per_hour: parse_decimal(row.try_get("rate")?, "rate")?,
                currency: row.try_get("currency")?,
            });
        }
        let data = ReferenceData { materials, rates };
        data.validate().map_err(|e| StoreError::Codec(e.to_string()))?;
        Ok(data)
    }

    /// Create a project and its first immutable version.
    pub async fn create_draft(&self, save: &SaveDraft) -> Result<SaveReceipt> {
        save.validate().map_err(|e| StoreError::Invalid(e.to_string()))?;
        if save.expected_version != 0 {
            return Err(StoreError::Invalid("a new project starts at version 0".into()));
        }
        let payload = payload_hash("create_project", &save.draft)?;
        let mut tx = self.pool.begin().await?;
        if let Some((stored, response)) = lookup_request(&mut tx, &save.request_id).await? {
            tx.commit().await?;
            if stored != payload {
                return Err(StoreError::Conflict("the request ID was reused with a different payload".into()));
            }
            return serde_json::from_value(response).map_err(StoreError::codec);
        }
        ensure_source(&mut tx, &save.draft.source).await?;

        let mut stored_draft = save.draft.clone();
        stored_draft.version = 1;
        let draft_json = serde_json::to_string(&stored_draft).map_err(StoreError::codec)?;
        sqlx::query("INSERT INTO projects (project_id, product_revision_id, current_version, source_id, currency) VALUES ($1, $2, 1, $3, $4)")
            .bind(stored_draft.project_id)
            .bind(stored_draft.product_revision_id)
            .bind(stored_draft.source.source_id)
            .bind(&stored_draft.currency)
            .execute(&mut *tx)
            .await?;
        insert_version(&mut tx, &stored_draft, &draft_json).await?;

        let receipt = SaveReceipt { request_id: save.request_id.clone(), project_id: stored_draft.project_id, version: 1 };
        insert_request(&mut tx, &save.request_id, "create_project", &payload, &receipt).await?;
        tx.commit().await?;
        Ok(receipt)
    }

    /// Save a new immutable version, if the client's expected version is still current.
    pub async fn save_draft(&self, save: &SaveDraft) -> Result<SaveReceipt> {
        save.validate().map_err(|e| StoreError::Invalid(e.to_string()))?;
        if save.expected_version == 0 {
            return Err(StoreError::Invalid("an existing project saves from version 1".into()));
        }
        let payload = payload_hash("save_draft", &save.draft)?;
        let mut tx = self.pool.begin().await?;
        if let Some((stored, response)) = lookup_request(&mut tx, &save.request_id).await? {
            tx.commit().await?;
            if stored != payload {
                return Err(StoreError::Conflict("the request ID was reused with a different payload".into()));
            }
            return serde_json::from_value(response).map_err(StoreError::codec);
        }

        let project = sqlx::query("SELECT current_version, source_id, product_revision_id FROM projects WHERE project_id = $1 FOR UPDATE")
            .bind(save.draft.project_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(StoreError::NotFound)?;
        let current: i64 = project.try_get("current_version")?;
        let current = u64::try_from(current).map_err(StoreError::codec)?;
        let project_source: Uuid = project.try_get("source_id")?;
        let product_revision: Uuid = project.try_get("product_revision_id")?;
        if save.draft.source.source_id != project_source {
            return Err(StoreError::Conflict("the draft belongs to a different source revision".into()));
        }
        if save.draft.product_revision_id != product_revision {
            return Err(StoreError::Conflict("the draft belongs to a different product revision".into()));
        }
        if current != save.expected_version {
            return Err(StoreError::Conflict(format!("the project is at version {current}, not {}", save.expected_version)));
        }
        ensure_source(&mut tx, &save.draft.source).await?;

        let version = current + 1;
        let mut stored_draft = save.draft.clone();
        stored_draft.version = version;
        let draft_json = serde_json::to_string(&stored_draft).map_err(StoreError::codec)?;
        insert_version(&mut tx, &stored_draft, &draft_json).await?;
        sqlx::query("UPDATE projects SET current_version = $2 WHERE project_id = $1")
            .bind(stored_draft.project_id)
            .bind(i64::try_from(version).map_err(StoreError::codec)?)
            .execute(&mut *tx)
            .await?;

        let receipt = SaveReceipt { request_id: save.request_id.clone(), project_id: stored_draft.project_id, version };
        insert_request(&mut tx, &save.request_id, "save_draft", &payload, &receipt).await?;
        tx.commit().await?;
        Ok(receipt)
    }

    /// The current draft.
    pub async fn load_project(&self, project_id: Uuid) -> Result<DraftProject> {
        let row = sqlx::query(
            "SELECT pv.draft::text AS draft FROM projects p \
             JOIN project_versions pv ON pv.project_id = p.project_id AND pv.version = p.current_version \
             WHERE p.project_id = $1",
        )
        .bind(project_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(StoreError::NotFound)?;
        let text: String = row.try_get("draft")?;
        serde_json::from_str(&text).map_err(StoreError::codec)
    }

    /// The current draft and the immutable calculations saved for the project.
    pub async fn project_view(&self, project_id: Uuid) -> Result<ProjectView> {
        let draft = self.load_project(project_id).await?;
        let rows = sqlx::query(
            "SELECT calculation_id, saved_version, calculator_version FROM calculations \
             WHERE project_id = $1 ORDER BY created_at, calculation_id",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?;
        let mut calculations = Vec::with_capacity(rows.len());
        for row in &rows {
            calculations.push(CalculationSummary {
                calculation_id: row.try_get("calculation_id")?,
                saved_version: u64::try_from(row.try_get::<i64, _>("saved_version")?).map_err(StoreError::codec)?,
                calculator_version: row.try_get("calculator_version")?,
            });
        }
        Ok(ProjectView { draft, calculations })
    }

    /// How many immutable versions a project holds (diagnostics and tests).
    pub async fn version_count(&self, project_id: Uuid) -> Result<u64> {
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM project_versions WHERE project_id = $1").bind(project_id).fetch_one(&self.pool).await?;
        u64::try_from(count).map_err(StoreError::codec)
    }

    async fn source_by_sha256(&self, sha256: &str) -> Result<Option<SourceRef>> {
        let row = sqlx::query("SELECT source_id, sha256, byte_len, original_name FROM sources WHERE sha256 = $1")
            .bind(sha256)
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref().map(row_to_source).transpose()
    }
}

fn row_to_source(row: &PgRow) -> Result<SourceRef> {
    Ok(SourceRef {
        source_id: row.try_get("source_id")?,
        sha256: row.try_get("sha256")?,
        byte_len: u64::try_from(row.try_get::<i64, _>("byte_len")?).map_err(StoreError::codec)?,
        original_name: row.try_get("original_name")?,
    })
}

fn parse_decimal(text: String, what: &str) -> Result<Decimal> {
    Decimal::from_str(&text).map_err(|e| StoreError::Codec(format!("{what}: {e}")))
}

async fn ensure_source(tx: &mut PgConnection, source: &SourceRef) -> Result<()> {
    source.validate().map_err(|e| StoreError::Invalid(e.to_string()))?;
    let row = sqlx::query("SELECT sha256, byte_len FROM sources WHERE source_id = $1")
        .bind(source.source_id)
        .fetch_optional(tx)
        .await?
        .ok_or_else(|| StoreError::Invalid("the source was not uploaded before the project".into()))?;
    let sha256: String = row.try_get("sha256")?;
    let byte_len = u64::try_from(row.try_get::<i64, _>("byte_len")?).map_err(StoreError::codec)?;
    if sha256 != source.sha256 || byte_len != source.byte_len {
        return Err(StoreError::Invalid("the source reference does not match the stored file".into()));
    }
    Ok(())
}

async fn insert_version(tx: &mut PgConnection, draft: &DraftProject, draft_json: &str) -> Result<()> {
    sqlx::query("INSERT INTO project_versions (project_id, version, draft, source_id) VALUES ($1, $2, $3::jsonb, $4)")
        .bind(draft.project_id)
        .bind(i64::try_from(draft.version).map_err(StoreError::codec)?)
        .bind(draft_json)
        .bind(draft.source.source_id)
        .execute(tx)
        .await?;
    Ok(())
}

async fn lookup_request(tx: &mut PgConnection, request_id: &str) -> Result<Option<(String, serde_json::Value)>> {
    let row = sqlx::query("SELECT payload_hash, response::text AS response FROM requests WHERE request_id = $1")
        .bind(request_id)
        .fetch_optional(tx)
        .await?;
    let Some(row) = row else { return Ok(None) };
    let hash: String = row.try_get("payload_hash")?;
    let text: String = row.try_get("response")?;
    let response = serde_json::from_str(&text).map_err(StoreError::codec)?;
    Ok(Some((hash, response)))
}

async fn insert_request(tx: &mut PgConnection, request_id: &str, operation: &str, payload: &str, response: &SaveReceipt) -> Result<()> {
    let response = serde_json::to_string(response).map_err(StoreError::codec)?;
    sqlx::query("INSERT INTO requests (request_id, operation, payload_hash, response) VALUES ($1, $2, $3, $4::jsonb)")
        .bind(request_id)
        .bind(operation)
        .bind(payload)
        .bind(response)
        .execute(tx)
        .await?;
    Ok(())
}

/// The canonical hash of a validated draft, not of the raw request bytes: key order and transport
/// details do not decide whether a retry is the same save.
fn payload_hash(operation: &str, draft: &DraftProject) -> Result<String> {
    let bytes = serde_json::to_vec(&(operation, draft)).map_err(StoreError::codec)?;
    Ok(hex::encode(Sha256::digest(&bytes)))
}
