//! Native hosted MCP transport and per-connection OAuth credentials.

use std::{
    collections::BTreeMap,
    sync::{Arc, mpsc},
    time::Duration,
};

use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, ClientConfig},
    transport::{
        StreamableHttpClientTransport,
        auth::{AuthClient, AuthError, AuthorizationManager, AuthorizationRequest, CredentialStore, OAuthState, StoredCredentials},
        streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Value, json};

type Reply = mpsc::Sender<Result<Value, String>>;

pub fn service() -> Option<solidcraft_ui_egui::requirements_panel::IssueService> {
    let (sender, receiver) = mpsc::sync_channel::<(Value, Value, Reply)>(8);
    std::thread::Builder::new()
        .name("issue-system".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build();
            for (config, operation, reply) in receiver {
                let result = match &runtime {
                    Ok(runtime) => runtime.block_on(async {
                        let seconds = if operation["kind"] == "connect" { 240 } else { 90 };
                        tokio::time::timeout(Duration::from_secs(seconds), request(config, operation))
                            .await
                            .unwrap_or_else(|_| Err("Issue-system connection timed out".into()))
                    }),
                    Err(_) => Err("Could not start issue-system networking".into()),
                };
                let _ = reply.send(result);
            }
        })
        .ok()?;
    Some(Arc::new(move |config, operation| {
        let (reply, response) = mpsc::channel();
        if let Err(error) = sender.try_send((config, operation, reply)) {
            let (mpsc::TrySendError::Full((_, _, reply)) | mpsc::TrySendError::Disconnected((_, _, reply))) = error;
            let _ = reply.send(Err("Issue-system connection is busy or unavailable".into()));
        }
        response
    }))
}

fn endpoint(value: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(value).map_err(|_| "Enter a valid HTTPS MCP URL")?;
    if value.len() > 2048
        || url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("Use an HTTPS MCP URL without credentials, query or fragment".into());
    }
    Ok(url)
}

fn account(config: &Value, url: &reqwest::Url) -> Result<String, String> {
    let id = config["id"].as_str().ok_or("Missing connection identity")?;
    let workspace = config["workspace"].as_str().ok_or("Missing workspace")?;
    if id.len() != 32 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) || workspace.is_empty() || workspace.len() > 128 {
        return Err("Invalid connection identity or workspace".into());
    }
    Ok(json!([id, url.as_str(), workspace]).to_string())
}

#[derive(Clone)]
struct Keychain(String);

impl Keychain {
    fn entry(&self) -> Result<keyring::Entry, AuthError> {
        keyring::Entry::new("SolidCraft issue systems", &self.0).map_err(|_| store_error())
    }
}

fn store_error() -> AuthError {
    AuthError::CredentialStoreError("The system keychain could not store or read this connection".into())
}

#[async_trait::async_trait]
impl CredentialStore for Keychain {
    async fn load(&self) -> Result<Option<StoredCredentials>, AuthError> {
        match self.entry()?.get_password() {
            Ok(value) => serde_json::from_str(&value).map(Some).map_err(|_| store_error()),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(store_error()),
        }
    }

    async fn save(&self, credentials: StoredCredentials) -> Result<(), AuthError> {
        let value = serde_json::to_string(&credentials).map_err(|_| store_error())?;
        self.entry()?.set_password(&value).map_err(|_| store_error())
    }

    async fn clear(&self) -> Result<(), AuthError> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(store_error()),
        }
    }
}

fn auth_error(error: AuthError) -> String {
    match error {
        AuthError::CredentialStoreError(_) => "System keychain unavailable; connection was not saved".into(),
        AuthError::RegistrationFailed(_) | AuthError::NoAuthorizationSupport => {
            "This server does not support automatic OAuth client registration".into()
        }
        AuthError::AuthorizationRequired | AuthError::TokenExpired | AuthError::TokenRefreshRejected(_) => "Sign in to this connection again".into(),
        AuthError::TokenRefreshFailed(_) => "Could not renew the session; retrying later".into(),
        _ => "OAuth authorization failed; check the server URL and sign in again".into(),
    }
}

async fn authorize(url: &reqwest::Url, store: Keychain) -> Result<AuthorizationManager, String> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.map_err(|_| "Could not open a local sign-in callback")?;
    let address = listener.local_addr().map_err(|_| "Could not read the local sign-in callback")?;
    let redirect = format!("http://127.0.0.1:{}/callback", address.port());
    let mut manager = AuthorizationManager::new(url.as_str()).await.map_err(auth_error)?;
    manager.set_credential_store(store);
    let mut state = OAuthState::Unauthorized(manager);
    let probe = StreamableHttpClientTransport::from_uri(url.as_str());
    let challenge = match tokio::time::timeout(Duration::from_secs(20), ClientConfig::default().serve(probe)).await {
        Ok(Err(error)) => error.auth_challenge().map(str::to_string),
        Ok(Ok(service)) => {
            let _ = service.cancel().await;
            None
        }
        Err(_) => return Err("MCP server did not respond to authorization discovery".into()),
    };
    let mut request =
        AuthorizationRequest::new(&redirect).with_client_name("SolidCraft").with_application_type("native").with_initial_scope_selection();
    if let Some(challenge) = challenge {
        request = request.with_challenge(challenge);
    }
    state.start_authorization(request).await.map_err(auth_error)?;
    let authorization_url = state.get_authorization_url().await.map_err(auth_error)?;
    let parsed = endpoint_authorization(&authorization_url)?;
    let (sender, mut receiver) = tokio::sync::mpsc::channel::<BTreeMap<String, String>>(1);
    let router = axum::Router::new().route(
        "/callback",
        axum::routing::get(move |axum::extract::Query(query): axum::extract::Query<BTreeMap<String, String>>| {
            let sender = sender.clone();
            async move {
                if sender.try_send(query).is_err() {
                    return (axum::http::StatusCode::CONFLICT, "Sign-in callback already received");
                }
                (axum::http::StatusCode::OK, "Return to SolidCraft to check the sign-in result. You may close this tab.")
            }
        }),
    );
    let mut server = tokio::task::JoinSet::new();
    server.spawn(async move { axum::serve(listener, router).await });
    let browser = std::process::Command::new(if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "xdg-open"
    })
    .arg(parsed.as_str())
    .spawn();
    if browser.is_err() {
        server.abort_all();
        return Err("Could not open the browser for sign-in".into());
    }
    let callback = tokio::time::timeout(Duration::from_secs(180), receiver.recv()).await;
    server.abort_all();
    let callback = callback.map_err(|_| "Sign-in timed out")?.ok_or("Sign-in cancelled")?;
    let code = callback.get("code").ok_or("Sign-in was not approved")?;
    let csrf = callback.get("state").ok_or("Sign-in callback omitted its security state")?;
    state.handle_callback_with_issuer(code, csrf, callback.get("iss").map(String::as_str)).await.map_err(auth_error)?;
    state.into_authorization_manager().ok_or_else(|| "Sign-in did not produce an authenticated connection".into())
}

fn endpoint_authorization(value: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(value).map_err(|_| "Invalid authorization URL")?;
    if url.scheme() != "https" || url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() {
        return Err("The authorization server must use HTTPS".into());
    }
    Ok(url)
}

async fn request(config: Value, operation: Value) -> Result<Value, String> {
    let mut config = config;
    if operation["kind"] == "connect" && config["id"].is_null() {
        let mut bytes = [0_u8; 16];
        getrandom::fill(&mut bytes).map_err(|_| "Could not create a connection identity")?;
        config["id"] = json!(bytes.iter().map(|byte| format!("{byte:02x}")).collect::<String>());
    }
    let url = endpoint(config["url"].as_str().ok_or("Missing MCP URL")?)?;
    let store = Keychain(account(&config, &url)?);
    let kind = operation["kind"].as_str().ok_or("Missing issue-system operation")?;
    if kind == "disconnect" {
        store.clear().await.map_err(auth_error)?;
        return Ok(json!({"disconnected": true}));
    }
    let manager = if kind == "connect" {
        tokio::time::timeout(Duration::from_secs(210), authorize(&url, store)).await.map_err(|_| "Sign-in timed out")??
    } else {
        let mut manager = AuthorizationManager::new(url.as_str()).await.map_err(auth_error)?;
        manager.set_credential_store(store);
        let resolution = manager.resolve_metadata().await.map_err(auth_error)?;
        manager.set_metadata(resolution.metadata);
        if !manager.initialize_from_store().await.map_err(auth_error)? {
            return Err("Sign in to this connection".into());
        }
        manager
    };
    let client = reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| "Could not configure HTTPS")?;
    let transport =
        StreamableHttpClientTransport::with_client(AuthClient::new(client, manager), StreamableHttpClientTransportConfig::with_uri(url.as_str()));
    let service = tokio::time::timeout(Duration::from_secs(30), ClientConfig::default().serve(transport))
        .await
        .map_err(|_| "MCP server did not respond")?
        .map_err(|_| "MCP connection failed; check the URL and sign-in")?;
    let result = tokio::time::timeout(Duration::from_secs(30), async {
        let tools = service.peer().list_all_tools().await.map_err(|_| "Could not read MCP tools")?;
        if kind == "connect" {
            if !tools.iter().any(|tool| tool.name == "get_issue") {
                return Err("This server has no supported issue reader".into());
            }
            return Ok(json!({"config": config, "tools": tools}));
        }
        let name = match kind {
            "read" => "get_issue",
            "comment" if config["publish_reviews"] == true => "create_comment",
            _ => return Err("This issue-system operation is not permitted".into()),
        };
        let tool = tools.iter().find(|tool| tool.name == name).ok_or("The connected server does not expose the required issue tool")?;
        let issue = operation["issue"].as_str().filter(|id| !id.is_empty() && id.len() <= 128).ok_or("Invalid issue identity")?;
        let arguments = if kind == "read" {
            json!({"id": issue})
        } else {
            let body = operation["body"].as_str().filter(|body| body.len() <= 60_000).ok_or("Review report is too large")?;
            json!({"issueId": issue, "body": body})
        };
        let arguments = arguments.as_object().cloned().ok_or("Invalid tool arguments")?;
        validate_arguments(&tool.input_schema, &arguments)?;
        if kind == "comment" {
            let read = tools.iter().find(|tool| tool.name == "get_issue").ok_or("Cannot verify the issue before publication")?;
            let args = json!({"id": issue}).as_object().cloned().ok_or("Invalid issue identity")?;
            validate_arguments(&read.input_schema, &args)?;
            let current = service
                .peer()
                .call_tool(CallToolRequestParams::new("get_issue").with_arguments(args))
                .await
                .map_err(|_| "Could not check the issue before publication")?;
            let current = tool_payload(&serde_json::to_value(current).map_err(|_| "Invalid issue response")?)?;
            validate_issue(&current, operation["issue_url"].as_str().ok_or("Missing issue URL")?)?;
            validate_publication_basis(&operation["spec"], &current)?;
            let comments =
                tools.iter().find(|tool| tool.name == "list_comments").ok_or("Automatic publication requires the server's comment reader")?;
            let args = json!({"issueId": issue}).as_object().cloned().ok_or("Invalid issue identity")?;
            validate_arguments(&comments.input_schema, &args)?;
            let comments = service
                .peer()
                .call_tool(CallToolRequestParams::new("list_comments").with_arguments(args))
                .await
                .map_err(|_| "Could not check existing reports")?;
            let comments = tool_payload(&serde_json::to_value(comments).map_err(|_| "Invalid comment response")?)?;
            let marker = operation["marker"]
                .as_str()
                .filter(|marker| marker.starts_with("solidcraft-review:") && marker.len() == 82)
                .ok_or("Invalid review marker")?;
            if already_published(&comments, marker)? {
                return Ok(json!({"published":true, "already_present":true}));
            }
        }
        let result =
            service.peer().call_tool(CallToolRequestParams::new(name).with_arguments(arguments)).await.map_err(|_| "Issue-system request failed")?;
        let result = serde_json::to_value(result).map_err(|_| "Invalid issue-system response")?;
        let payload = tool_payload(&result)?;
        if kind == "read" {
            validate_issue(&payload, operation["issue_url"].as_str().ok_or("Missing expected issue URL")?)?;
        }
        Ok(payload)
    })
    .await
    .map_err(|_| "Issue-system request timed out");
    let _ = tokio::time::timeout(Duration::from_secs(2), service.cancel()).await;
    result?
}

fn validate_issue(issue: &Value, expected: &str) -> Result<(), String> {
    let url = endpoint(issue["url"].as_str().ok_or("Issue response omitted its URL")?)?;
    let expected = endpoint(expected)?;
    if url.host_str() != Some("linear.app")
        || expected.host_str() != Some("linear.app")
        || url.path().split('/').nth(1) != expected.path().split('/').nth(1)
        || url.path().split('/').nth(2) != Some("issue")
        || expected.path().split('/').nth(2) != Some("issue")
        || url.path().split('/').nth(3) != expected.path().split('/').nth(3)
    {
        return Err("The issue belongs to a different workspace or issue; synchronization is paused".into());
    }
    Ok(())
}

fn already_published(comments: &Value, marker: &str) -> Result<bool, String> {
    let items = comments.as_array().or_else(|| comments["comments"].as_array()).ok_or("Unsupported comment-list response; publication paused")?;
    if items.iter().any(|comment| comment["body"].as_str().is_some_and(|body| body.contains(marker))) {
        return Ok(true);
    }
    let mut complete = comments.is_array();
    for flag in [comments.get("hasMore"), comments.get("hasNextPage"), comments["pageInfo"].get("hasNextPage")].into_iter().flatten() {
        if flag != false {
            return Err("Comment list is incomplete; automatic publication paused".into());
        }
        complete = true;
    }
    if !complete || comments.get("nextCursor").is_some_and(|cursor| !cursor.is_null()) {
        return Err("Comment-list completeness is unknown; automatic publication paused".into());
    }
    Ok(false)
}

fn validate_publication_basis(spec: &Value, issue: &Value) -> Result<(), String> {
    let requirements = spec["requirements"].as_array().ok_or("Missing publication basis")?;
    let Some(requirement) = requirements.first().filter(|_| requirements.len() == 1) else {
        return Err("Automatic publication requires a single directly linked issue".into());
    };
    if requirement["title"] != issue["title"]
        || requirement["description"].as_str().unwrap_or_default() != issue["description"].as_str().unwrap_or_default()
    {
        return Err("The issue changed before publication; wait for refresh and review the current requirements".into());
    }
    Ok(())
}

fn validate_arguments(schema: &serde_json::Map<String, Value>, arguments: &serde_json::Map<String, Value>) -> Result<(), String> {
    let properties = schema.get("properties").and_then(Value::as_object).ok_or("Unsupported issue-tool schema")?;
    let required = schema.get("required").and_then(Value::as_array);
    if required.is_some_and(|required| required.iter().any(|key| key.as_str().is_none_or(|key| !arguments.contains_key(key))))
        || arguments.iter().any(|(key, value)| {
            let Some(property) = properties.get(key) else { return true };
            property["type"] != "string"
                || !value.is_string()
                || property["maxLength"].as_u64().is_some_and(|max| value.as_str().is_some_and(|text| text.chars().count() as u64 > max))
        })
    {
        return Err("The issue-tool schema has changed; automatic synchronization is paused".into());
    }
    Ok(())
}

fn tool_payload(result: &Value) -> Result<Value, String> {
    if result["isError"] == true {
        return Err("The issue provider rejected the operation".into());
    }
    if let Some(value) = result.get("structuredContent").filter(|value| value.is_object()) {
        return Ok(value.clone());
    }
    let text = result["content"]
        .as_array()
        .and_then(|items| items.iter().find_map(|item| item["text"].as_str()))
        .ok_or("Issue response contained no structured data")?;
    if text.len() > 1_048_576 {
        return Err("Issue response is too large".into());
    }
    serde_json::from_str(text).map_err(|_| "Issue response is not valid JSON".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_connections_reject_insecure_or_secret_bearing_urls() {
        for url in [
            "http://mcp.linear.app/mcp",
            "https://token@mcp.linear.app/mcp",
            "https://mcp.linear.app/mcp?token=secret",
            "file:///tmp/mcp",
            "https://mcp.linear.app/mcp#token",
        ] {
            assert!(endpoint(url).is_err());
        }
        assert!(endpoint("https://mcp.linear.app/mcp").is_ok());
        assert!(endpoint_authorization("http://example.com/authorize?code=secret").is_err());
    }

    #[test]
    fn issue_connection_credentials_are_isolated_by_connection_workspace_and_server() {
        let url = endpoint("https://mcp.linear.app/mcp").unwrap();
        let config = json!({"id":"0123456789abcdef0123456789abcdef", "workspace":"one"});
        let first = account(&config, &url).unwrap();
        let mut second = config.clone();
        second["workspace"] = json!("two");
        assert_ne!(first, account(&second, &url).unwrap());
        second["workspace"] = json!("one");
        second["id"] = json!("1123456789abcdef0123456789abcdef");
        assert_ne!(first, account(&second, &url).unwrap());
        assert_ne!(first, account(&config, &endpoint("https://example.com/mcp").unwrap()).unwrap());
        assert!(!auth_error(AuthError::TokenExchangeFailed("secret".into())).contains("secret"));
    }

    #[test]
    fn issue_tool_errors_and_schema_changes_cannot_be_reported_as_success() {
        assert!(tool_payload(&json!({"isError":true,"content":[{"text":"secret failure"}]})).is_err());
        assert_eq!(tool_payload(&json!({"content":[{"text":"{\"title\":\"One\"}"}]})).unwrap()["title"], "One");
        let schema = json!({"properties":{"id":{"type":"string"}},"required":["id"]});
        assert!(validate_arguments(schema.as_object().unwrap(), json!({"id":"EX-1"}).as_object().unwrap()).is_ok());
        assert!(validate_arguments(schema.as_object().unwrap(), json!({"issueId":"EX-1"}).as_object().unwrap()).is_err());
        assert!(validate_arguments(schema.as_object().unwrap(), json!({}).as_object().unwrap()).is_err());
        assert!(validate_issue(&json!({"url":"https://linear.app/other/issue/EX-1/test"}), "https://linear.app/example/issue/EX-1/title").is_err());
        assert!(validate_publication_basis(&json!({"requirements":[{"title":"Old"}]}), &json!({"title":"New"})).is_err());
    }

    #[test]
    fn publication_checks_require_complete_comments_or_an_existing_marker() {
        let marker = "solidcraft-review:example";
        assert_eq!(already_published(&json!([]), marker), Ok(false));
        assert_eq!(already_published(&json!({"comments":[], "hasNextPage":false}), marker), Ok(false));
        for comments in [
            json!({"comments":[], "hasNextPage":true}),
            json!({"comments":[], "hasMore":true}),
            json!({"comments":[], "pageInfo":{"hasNextPage":true}}),
            json!({"comments":[], "hasNextPage":false, "nextCursor":"next"}),
            json!({"comments":[]}),
            json!({"comments":[], "hasNextPage":"unknown"}),
        ] {
            assert!(already_published(&comments, marker).is_err(), "{comments}");
        }
        assert_eq!(already_published(&json!({"comments":[{"body":marker}], "hasNextPage":true}), marker), Ok(true));
    }
}
