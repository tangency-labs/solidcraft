//! OS keychain and native HTTP for guided setup. Neither secrets nor HTTP reach the engine.
use bytes::Bytes;
use http::{Request, Response};
use http_body_util::{BodyExt, Full};
use octocrab::{
    AuthState, OctoBody, OctocrabBuilder,
    service::middleware::{auth_header::AuthHeaderLayer, base_uri::BaseUriLayer},
};
use solidcraft_cloud::{Connection, Error, Result, jobs::Job};
use solidcraft_engine::io::cloud::NativeAccess;
use std::{io::Read, sync::Arc};
use tower::{BoxError, service_fn};

pub struct Access;

fn storage() -> Error {
    Error::Storage("system credential store unavailable; unlock it and reconnect".into())
}

pub(super) fn entry(id: &str) -> Result<keyring::Entry> {
    if id.is_empty() || id.len() > 4096 || id.chars().any(char::is_control) {
        return Err(storage());
    }
    keyring::Entry::new("SolidCraft cloud saves", id).map_err(|_| storage())
}

impl NativeAccess for Access {
    fn credential(&self, id: &str) -> Result<Option<String>> {
        match entry(id)?.get_password() {
            Ok(token) => Ok(Some(token)),
            Err(keyring::Error::NoEntry) => Err(Error::Storage("repository token is missing; reconnect this connection".into())),
            Err(_) => Err(storage()),
        }
    }
    fn store_credential(&self, id: &str, token: &str) -> Result<()> {
        solidcraft_cloud::setup::check_token(token)?;
        entry(id)?.set_password(token).map_err(|_| storage())
    }
    fn store_ssh_passphrase(&self, path: &str, passphrase: &str) -> Result<()> {
        entry(&solidcraft_cloud::jobs::ssh_credential(path))?.set_password(passphrase).map_err(|_| storage())
    }
    fn ssh_askpass(&self, path: &str) -> Result<Option<std::path::PathBuf>> {
        // Existing keys continue using system authentication when no saved secret is
        // available. Only a key with an accessible entry opts into our helper.
        match entry(&solidcraft_cloud::jobs::ssh_credential(path))?.get_password() {
            Ok(secret) => {
                let _secret = zeroize::Zeroizing::new(secret);
                std::env::current_exe().map(Some).map_err(|_| storage())
            }
            Err(_) => Ok(None),
        }
    }
    fn prepare(&self, connection: Connection, job: &Job) -> Result<Connection> {
        let token = self.credential(&connection.credential)?.ok_or_else(storage)?;
        let runtime = tokio::runtime::Builder::new_current_thread().build().map_err(|_| Error::Storage("GitHub worker unavailable".into()))?;
        tokio::task::LocalSet::new().block_on(&runtime, async {
            let client = client(&token, job.clone()).map_err(|_| Error::Storage("GitHub client unavailable".into()))?;
            solidcraft_cloud::github::prepare(client, connection).await
        })
    }
}

fn client(token: &str, job: Job) -> std::result::Result<octocrab::Octocrab, BoxError> {
    solidcraft_cloud::setup::check_token(token)?;
    let tls = ureq::tls::TlsConfig::builder()
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .build();
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .tls_config(tls)
        .http_status_as_error(false)
        .user_agent("SolidCraft")
        .max_redirects(0)
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .build()
        .into();
    let agent = Arc::new(agent);
    let service = service_fn(move |request: Request<OctoBody>| {
        let agent = agent.clone();
        let job = job.clone();
        async move {
            if job.cancelled()? {
                return Err(BoxError::from("setup cancelled"));
            }
            let (parts, body) = request.into_parts();
            if parts.uri.scheme_str() != Some("https") || parts.uri.host() != Some("api.github.com") {
                return Err(BoxError::from("unsupported API destination"));
            }
            let bytes = body.collect().await?.to_bytes();
            if bytes.len() > solidcraft_cloud::MAX_MANIFEST_BYTES {
                return Err("request too large".into());
            }
            let mut builder = http::Request::builder().method(parts.method).uri(parts.uri);
            if let Some(headers) = builder.headers_mut() {
                *headers = parts.headers;
                headers.entry(http::header::ACCEPT).or_insert(http::HeaderValue::from_static("application/vnd.github+json"));
            }
            let mut response = agent.run(builder.body(bytes.to_vec())?)?;
            let mut result = Response::builder().status(response.status());
            if let Some(headers) = result.headers_mut() {
                *headers = response.headers().clone();
            }
            let mut output = Vec::new();
            response.body_mut().as_reader().take(1024 * 1024 + 1).read_to_end(&mut output)?;
            if output.len() > 1024 * 1024 {
                return Err("response too large".into());
            }
            if job.cancelled()? {
                return Err(BoxError::from("setup cancelled"));
            }
            Ok::<_, BoxError>(result.body(Full::new(Bytes::from(output)))?)
        }
    });
    let api = "https://api.github.com".parse()?;
    let mut header = http::HeaderValue::from_str(&format!("Bearer {token}"))?;
    header.set_sensitive(true);
    OctocrabBuilder::new_empty()
        .with_service(service)
        .with_layer(&BaseUriLayer::new(api))
        .with_layer(&AuthHeaderLayer::new(Some(header), "https://api.github.com".parse()?, "https://api.github.com".parse()?))
        .with_auth(AuthState::None)
        .with_executor(Box::new(|future| {
            tokio::task::spawn_local(future);
        }))
        .build()
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_octocrab_client_uses_its_local_executor_without_a_global_runtime() {
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        tokio::task::LocalSet::new().block_on(&runtime, async {
            let job = Job::new("test".into()).unwrap();
            let client = client("test-token", job).unwrap();
            drop(client);
            tokio::task::yield_now().await;
        });
    }
}
