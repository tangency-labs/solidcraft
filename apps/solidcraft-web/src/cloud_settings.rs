//! Machine-local connection metadata and provider credentials, outside design catalogs.
use indexed_db_futures::{
    database::Database,
    prelude::*,
    transaction::{TransactionDurability, TransactionMode, TransactionOptions},
};
use solidcraft_cloud::{Configuration, Error, Result, Settings};

use crate::cloud_storage::with_lock;

pub struct LocalSettings(Database);

fn storage(error: impl std::fmt::Display) -> Error {
    Error::Storage(format!("browser connection storage: {error}"))
}

impl Drop for LocalSettings {
    fn drop(&mut self) {
        self.0.clone().close();
    }
}

impl LocalSettings {
    pub async fn open() -> Result<Self> {
        let database = Database::open("solidcraft-cloud-settings")
            .with_on_upgrade_needed(|_, db| {
                db.create_object_store("settings").build()?;
                db.create_object_store("credentials").build()?;
                Ok(())
            })
            .await
            .map_err(storage)?;
        if database.version() != 1.0 {
            database.close();
            return Err(Error::UpgradeRequired("browser connection database version".into()));
        }
        Ok(Self(database))
    }

    async fn configuration(&self) -> Result<Configuration> {
        let transaction = self.0.transaction("settings").build().map_err(storage)?;
        let text: Option<String> = transaction.object_store("settings").map_err(storage)?.get("connections").await.map_err(storage)?;
        transaction.commit().await.map_err(storage)?;
        text.map(|text| Configuration::read(text.as_bytes())).transpose().map(|config| config.unwrap_or_default())
    }

    pub async fn settings(&self) -> Result<Settings> {
        self.configuration().await?.settings()
    }

    pub async fn configure(self, settings: Settings) -> Result<()> {
        settings.validate()?;
        with_lock("solidcraft-cloud-settings", async move {
            let bytes = self.configuration().await?.update(&settings)?;
            let text = String::from_utf8(bytes).map_err(storage)?;
            let transaction = self
                .0
                .transaction("settings")
                .with_mode(TransactionMode::Readwrite)
                .with_options(TransactionOptions::new().with_durability(TransactionDurability::Strict))
                .build()
                .map_err(storage)?;
            transaction.object_store("settings").map_err(storage)?.put(text).with_key("connections").await.map_err(storage)?;
            transaction.commit().await.map_err(storage)
        })
        .await
    }

    pub async fn credential(&self, id: &str) -> Result<Option<String>> {
        credential_id(id)?;
        let transaction = self.0.transaction("credentials").build().map_err(storage)?;
        let text: Option<String> = transaction.object_store("credentials").map_err(storage)?.get(id).await.map_err(storage)?;
        transaction.commit().await.map_err(storage)?;
        let Some(text) = text else { return Ok(None) };
        if text.len() > 16_384 {
            return Err(storage("credential record exceeds the safe size limit"));
        }
        let value: serde_json::Value = serde_json::from_str(&text).map_err(|_| storage("credential record is damaged"))?;
        if value.get("format").and_then(serde_json::Value::as_str) != Some("solidcraft-credential/1") {
            return Err(Error::UpgradeRequired("browser credential format".into()));
        }
        let token = value.get("value").and_then(serde_json::Value::as_str).ok_or_else(|| storage("credential record is damaged"))?;
        credential_value(token)?;
        Ok(Some(token.to_owned()))
    }

    /// Setup calls this directly, keeping token bytes out of engine command parameters/logs.
    /// Disconnecting a connection does not automatically discard queued designs or secrets.
    pub async fn store_credential(self, id: String, token: Option<String>) -> Result<()> {
        credential_id(&id)?;
        if let Some(token) = &token {
            credential_value(token)?;
        }
        with_lock("solidcraft-cloud-credentials", async move {
            // Refuse an unsupported existing record rather than quietly replacing it.
            self.credential(&id).await?;
            let transaction = self
                .0
                .transaction("credentials")
                .with_mode(TransactionMode::Readwrite)
                .with_options(TransactionOptions::new().with_durability(TransactionDurability::Strict))
                .build()
                .map_err(storage)?;
            let store = transaction.object_store("credentials").map_err(storage)?;
            if let Some(token) = token {
                let record = serde_json::json!({"format":"solidcraft-credential/1","value":token}).to_string();
                store.put(record).with_key(id.as_str()).await.map_err(storage)?;
            } else {
                store.delete(id.as_str()).await.map_err(storage)?;
            }
            transaction.commit().await.map_err(storage)
        })
        .await
    }
}

fn credential_id(id: &str) -> Result<()> {
    if id.is_empty() || id.len() > 4096 || id.chars().any(char::is_control) {
        return Err(storage("invalid credential identifier"));
    }
    Ok(())
}

fn credential_value(token: &str) -> Result<()> {
    if token.is_empty() || token.len() > 8192 || !token.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(storage("invalid provider token"));
    }
    Ok(())
}
