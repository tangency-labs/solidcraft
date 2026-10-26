//! Desktop WebDAV transport using the existing HTTP client and OS credential store.
use solidcraft_ui_egui::i18n::tr;
use solidcraft_ui_egui::webdav::{Request, Response};

struct Slot(std::sync::Arc<std::sync::atomic::AtomicUsize>);
impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }
}

pub fn service() -> solidcraft_ui_egui::webdav::Http {
    let active = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    std::rc::Rc::new(move |mut request| {
        let previous = active.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let slot = Slot(active.clone());
        if previous >= 4 {
            return Box::pin(async { Err(tr("webdav.error.limit").into()) });
        }
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let language = solidcraft_ui_egui::i18n::language();
        let job = std::thread::Builder::new().name("webdav".into()).spawn(move || {
            let _slot = slot;
            let _language = solidcraft_ui_egui::i18n::scope(language);
            let result = (|| {
                solidcraft_ui_egui::webdav::connection(&request.url)?;
                if request.user.contains(':') || request.user.len() > 1024 || request.password.len() > 4096 {
                    return Err(tr("webdav.error.credentials").into());
                }
                let url = ureq::http::Uri::try_from(request.url.as_str()).map_err(|_| tr("webdav.error.url"))?;
                let id = format!("{}://{}:{}", url.scheme_str().unwrap_or("https"), url.authority().ok_or(tr("webdav.error.url"))?, request.user);
                let entry = keyring::Entry::new("SolidCraft WebDAV", &id).map_err(|_| tr("webdav.error.keychain"))?;
                let provided = (!request.password.is_empty()).then(|| request.password.clone());
                if request.password.is_empty() {
                    request.password = match entry.get_password() {
                        Ok(secret) => secret,
                        Err(keyring::Error::NoEntry) => String::new(),
                        Err(_) => return Err(tr("webdav.error.keychain").into()),
                    };
                }
                let response = send(request)?;
                if ((200..300).contains(&response.status) || response.status == 412)
                    && let Some(password) = provided
                {
                    entry.set_password(&password).map_err(|_| tr("webdav.error.keychain"))?;
                }
                Ok(response)
            })();
            let _ = tx.send(result);
        });
        Box::pin(async move {
            if job.is_err() {
                return Err(tr("webdav.error.worker").into());
            }
            std::future::poll_fn(move |_| match rx.try_recv() {
                Ok(answer) => std::task::Poll::Ready(answer),
                Err(std::sync::mpsc::TryRecvError::Empty) => std::task::Poll::Pending,
                Err(_) => std::task::Poll::Ready(Err(tr("webdav.error.worker").into())),
            })
            .await
        })
    })
}

pub fn send(r: Request) -> Result<Response, String> {
    solidcraft_ui_egui::webdav::connection(&r.url)?;
    if r.user.contains(':') || r.user.len() > 1024 || r.password.len() > 4096 || r.body.len() > solidcraft_engine::io::MAX_DESIGN_BYTES {
        return Err(tr("webdav.error.credentials").into());
    }
    let tls = ureq::tls::TlsConfig::builder()
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .unversioned_rustls_crypto_provider(std::sync::Arc::new(rustls::crypto::ring::default_provider()))
        .build();
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .tls_config(tls)
        .allow_non_standard_methods(true)
        .http_status_as_error(false)
        .max_redirects(0)
        .timeout_global(Some(std::time::Duration::from_secs(60)))
        .timeout_connect(Some(std::time::Duration::from_secs(10)))
        .build()
        .into();
    let auth = solidcraft_ui_egui::agent_chat::backend::base64(format!("{}:{}", r.user, r.password).as_bytes());
    let mut authorization = ureq::http::HeaderValue::from_str(&format!("Basic {auth}")).map_err(|_| tr("webdav.error.credentials"))?;
    authorization.set_sensitive(true);
    let mut request = ureq::http::Request::builder().method(r.method).uri(&r.url).header("Authorization", authorization);
    for (key, value) in r.headers {
        request = request.header(key, value);
    }
    let request = request.body(r.body).map_err(|_| tr("webdav.error.transport"))?;
    let mut response = agent.run(request).map_err(|_| tr("webdav.error.transport"))?;
    let status = response.status().as_u16();
    let etag = response.headers().get("ETag").and_then(|h| h.to_str().ok()).map(str::to_string);
    let body = if r.method == "PUT" || !(200..300).contains(&status) {
        Vec::new()
    } else {
        response
            .body_mut()
            .with_config()
            .limit(r.cap.min(solidcraft_engine::io::MAX_DESIGN_BYTES) as u64)
            .read_to_vec()
            .map_err(|_| tr("webdav.error.transport"))?
    };
    Ok(Response { status, etag, body })
}
