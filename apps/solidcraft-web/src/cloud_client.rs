//! Octocrab's public service/executor seam, backed by browser Fetch through reqwest.
//! Only owned Rust response bytes enter its Send service future. JS futures stay on the
//! browser task executor; GitHub request builders and response models remain in Octocrab.
use bytes::{Bytes, BytesMut};
use futures::{StreamExt, channel::oneshot};
use http::{Request, Response};
use http_body_util::{BodyExt, Full};
use octocrab::{
    AuthState, OctoBody, Octocrab, OctocrabBuilder,
    service::middleware::{auth_header::AuthHeaderLayer, base_uri::BaseUriLayer, extra_headers::ExtraHeadersLayer},
};
use std::sync::Arc;
use tower::{BoxError, service_fn};

const API: &str = "https://api.github.com";
const RESPONSE_LIMIT: usize = solidcraft_cloud::MAX_MANIFEST_BYTES * 2;

async fn fetch(request: Request<OctoBody>) -> Result<Response<Full<Bytes>>, BoxError> {
    let (parts, body) = request.into_parts();
    if parts.uri.scheme_str() != Some("https") || parts.uri.host() != Some("api.github.com") {
        return Err("unsupported GitHub API destination".into());
    }
    let body = body.collect().await?.to_bytes();
    if body.len() > RESPONSE_LIMIT {
        return Err("GitHub request exceeds the safety limit".into());
    }
    let timeout = if parts.uri.path().contains("/git/blobs") { 300 } else { 10 };
    let response = reqwest::Client::new()
        .request(parts.method, parts.uri.to_string())
        .headers(parts.headers)
        .body(body.to_vec())
        .timeout(std::time::Duration::from_secs(timeout))
        .send()
        .await?;
    let mut result = Response::builder().status(response.status());
    if let Some(headers) = result.headers_mut() {
        *headers = response.headers().clone();
    }
    let stream = response.bytes_stream();
    futures::pin_mut!(stream);
    let mut bytes = BytesMut::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if chunk.len() > RESPONSE_LIMIT.saturating_sub(bytes.len()) {
            return Err("GitHub response exceeds the safety limit".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(result.body(Full::new(bytes.freeze()))?)
}

// Dropping a failed/cancelled listing must also stop sibling Fetch tasks whose
// Send response receivers were dropped by the bounded stream.
struct RequestAbort(futures::future::AbortHandle);
impl Drop for RequestAbort {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub fn client(token: Option<&str>) -> Result<Octocrab, BoxError> {
    client_for_job(token, None)
}

pub fn client_for_job(token: Option<&str>, job: Option<solidcraft_cloud::jobs::Job>) -> Result<Octocrab, BoxError> {
    let service = service_fn(move |request: Request<OctoBody>| {
        let (sender, receiver) = oneshot::channel();
        let job = job.clone();
        let (handle, registration) = futures::future::AbortHandle::new_pair();
        let request_abort = RequestAbort(handle.clone());
        wasm_bindgen_futures::spawn_local(async move {
            let _cancellation = match job {
                Some(job) => match job.on_cancel(move || handle.abort()) {
                    Ok(guard) => Some(guard),
                    Err(error) => {
                        let _ = sender.send(Err(BoxError::from(error.to_string())));
                        return;
                    }
                },
                None => None,
            };
            // Dropping reqwest's Fetch future aborts its browser request. Durable OPFS/IDB
            // operations are outside this scope and always finish their acknowledgement.
            let result = futures::future::Abortable::new(fetch(request), registration)
                .await
                .unwrap_or_else(|_| Err(BoxError::from("browser request cancelled")));
            let _ = sender.send(result);
        });
        async move {
            let _request_abort = request_abort;
            receiver.await.map_err(|_| BoxError::from("browser request cancelled"))?
        }
    });
    let header = token
        .map(|token| -> Result<http::HeaderValue, BoxError> {
            if token.is_empty() || token.len() > 8192 {
                return Err("invalid repository token".into());
            }
            let mut value = http::HeaderValue::from_str(&format!("Bearer {token}"))?;
            value.set_sensitive(true);
            Ok(value)
        })
        .transpose()?;
    OctocrabBuilder::new_empty()
        .with_service(service)
        .with_layer(&BaseUriLayer::new(API.parse()?))
        .with_layer(&ExtraHeadersLayer::new(Arc::new(vec![(http::header::ACCEPT, "application/vnd.github+json".parse()?)])))
        .with_layer(&AuthHeaderLayer::new(header, API.parse()?, API.parse()?))
        // Octocrab supplies the API-version header. Adding it twice makes GitHub reject it.
        .with_auth(AuthState::None)
        .with_executor(Box::new(wasm_bindgen_futures::spawn_local))
        .build()
        .map_err(Into::into)
}
