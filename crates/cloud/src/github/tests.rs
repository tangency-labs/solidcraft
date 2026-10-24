use super::*;
use bytes::Bytes;
use http::{Request, Response};
use http_body_util::{BodyExt, Full};
use octocrab::{AuthState, OctoBody, OctocrabBuilder, service::middleware::base_uri::BaseUriLayer};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tower::{BoxError, service_fn};

const URL: &str = "https://api.github.com/repos/owner/designs";

#[derive(Default)]
struct Repository {
    files: BTreeMap<String, Vec<u8>>,
    blobs: BTreeMap<String, Vec<u8>>,
    modes: BTreeMap<String, String>,
    pending: BTreeMap<String, Vec<u8>>,
    commit: String,
    commits: usize,
    requests: Vec<(String, String, Value)>,
    compete_once: bool,
    empty: bool,
    readonly: bool,
    denied: bool,
    initialized: bool,
    write_denied: bool,
    automatic: bool,
}

impl Repository {
    fn reference(&self) -> Value {
        json!({"ref":"refs/heads/main","node_id":"ref","url":URL,"object":{"type":"commit","sha":self.commit,"url":URL}})
    }

    fn commit(&self, sha: &str) -> Value {
        let author = json!({"name":"Cloud tests","email":"cloud@example.invalid","date":"2026-10-22T00:00:00Z"});
        json!({"sha":sha,"node_id":"commit","url":URL,"html_url":URL,"author":author,"committer":author,
            "message":"Revision","tree":{"sha":"b".repeat(40),"url":URL},"parents":[],
            "verification":{"verified":false,"reason":"unsigned","signature":null,"payload":null}})
    }

    fn respond(&mut self, method: &str, path: &str, body: Value) -> (u16, Value) {
        self.requests.push((method.into(), path.into(), body.clone()));
        if method == "GET" && path == "/user" {
            if self.denied {
                return (401, json!({"message":"denied","documentation_url":URL}));
            }
            let mut account = json!({"login":"owner","id":1,"node_id":"user","gravatar_id":"","type":"User","site_admin":false});
            for key in [
                "avatar_url",
                "url",
                "html_url",
                "followers_url",
                "following_url",
                "gists_url",
                "starred_url",
                "subscriptions_url",
                "organizations_url",
                "repos_url",
                "events_url",
                "received_events_url",
            ] {
                account[key] = json!(URL);
            }
            return (200, account);
        }
        if method == "GET" && (path == "/repos/owner/designs" || (self.automatic && path == "/repos/owner/solidcraft-designs")) {
            if self.denied {
                return (403, json!({"message":"denied","documentation_url":URL}));
            }
            return (
                200,
                json!({"id":1,"name":"designs","full_name":if self.automatic {"owner/solidcraft-designs"} else {"owner/designs"},"url":URL,"default_branch":"main",
                "size":if self.empty {0} else {1},"permissions":{"push":!self.readonly,"pull":true},"private":true}),
            );
        }
        if method == "PUT" && path.ends_with("/contents/README.md") {
            assert!(self.empty && !self.initialized);
            assert_eq!(body["branch"], "main");
            assert!(body.get("sha").is_none(), "setup cannot overwrite an existing file");
            self.initialized = true;
            return (
                201,
                json!({"content":{"name":"README.md","path":"README.md","sha":"a".repeat(40),"size":1,"url":URL,"type":"file","_links":{"self":URL}},"commit":{}}),
            );
        }
        if method == "GET" && path.contains("/git/ref/") {
            if self.empty && !self.initialized {
                return (409, json!({"message":"empty repository","documentation_url":URL}));
            }
            return (200, self.reference());
        }
        if method == "GET" && path.contains("/git/commits/") {
            return (200, self.commit(&self.commit));
        }
        if method == "GET" && path.contains("/git/trees/") {
            let mut entries = Vec::new();
            for (path, bytes) in &self.files {
                let sha = blob_hash(bytes, 40).unwrap();
                self.blobs.insert(sha.clone(), bytes.clone());
                entries.push(json!({"path":path,"mode":self.modes.get(path).map(String::as_str).unwrap_or("100644"),
                    "type":"blob","sha":sha,"size":bytes.len()}));
            }
            return (200, json!({"sha":"b".repeat(40),"url":URL,"tree":entries,"truncated":false}));
        }
        if method == "GET" && path.contains("/git/blobs/") {
            let sha = path.rsplit('/').next().unwrap();
            let bytes = &self.blobs[sha];
            return (200, json!({"sha":sha,"url":URL,"node_id":"blob","content":STANDARD.encode(bytes),"encoding":"base64","size":bytes.len()}));
        }
        if method == "POST" && path.ends_with("/git/blobs") {
            assert_eq!(body["encoding"], "base64");
            let bytes = STANDARD.decode(body["content"].as_str().unwrap()).unwrap();
            let sha = blob_hash(&bytes, 40).unwrap();
            self.blobs.insert(sha.clone(), bytes);
            return (201, json!({"sha":sha,"url":URL}));
        }
        if method == "POST" && path.ends_with("/git/trees") {
            assert_eq!(body["base_tree"], "b".repeat(40));
            self.pending = self.files.clone();
            for entry in body["tree"].as_array().unwrap() {
                let path = entry["path"].as_str().unwrap();
                let sha = entry["sha"].as_str().unwrap();
                self.pending.insert(path.into(), self.blobs[sha].clone());
            }
            return (201, json!({"sha":"c".repeat(40),"url":URL,"tree":[],"truncated":false}));
        }
        if method == "POST" && path.ends_with("/git/commits") {
            assert_eq!(body["parents"], json!([self.commit]));
            self.commits += 1;
            return (201, self.commit(&format!("{:040x}", self.commits)));
        }
        if method == "PATCH" && path.contains("/git/refs/") {
            if self.write_denied {
                return (403, json!({"message":"Contents permission denied","documentation_url":URL}));
            }
            assert_eq!(body["force"], false);
            if self.compete_once {
                self.compete_once = false;
                let path = self.files.keys().find(|path| path.ends_with("/manifest.json")).unwrap().clone();
                let mut manifest: Value = serde_json::from_slice(&self.files[&path]).unwrap();
                manifest["revision"] = json!("external");
                self.files.insert(path, serde_json::to_vec(&manifest).unwrap());
                self.commit = "f".repeat(40);
                return (422, json!({"message":"Update is not a fast forward","documentation_url":URL}));
            }
            if body["sha"].as_str() == Some(self.commit.as_str()) {
                return (200, self.reference());
            }
            self.files = self.pending.clone();
            self.commit = body["sha"].as_str().unwrap().into();
            return (200, self.reference());
        }
        panic!("unexpected mock request: {method} {path}");
    }
}

fn adapter(repository: Arc<Mutex<Repository>>) -> (GitHub, Connection) {
    let service = service_fn(move |request: Request<OctoBody>| {
        let repository = repository.clone();
        async move {
            let (parts, body) = request.into_parts();
            let body = body.collect().await?.to_bytes();
            let body = if body.is_empty() { Value::Null } else { serde_json::from_slice(&body)? };
            let (status, response) = repository.lock().unwrap().respond(parts.method.as_str(), parts.uri.path(), body);
            Ok::<_, BoxError>(
                Response::builder()
                    .status(status)
                    .header("content-type", "application/json")
                    .body(Full::new(Bytes::from(serde_json::to_vec(&response)?)))?,
            )
        }
    });
    let client = OctocrabBuilder::new_empty()
        .with_service(service)
        .with_layer(&BaseUriLayer::new("https://api.github.com".parse().unwrap()))
        .with_auth(AuthState::None)
        .with_executor(Box::new(|future| {
            tokio::task::spawn_local(future);
        }))
        .build()
        .unwrap();
    let connection = Connection { repository: "owner/designs".into(), ..Default::default() };
    (GitHub::new(client, &connection).unwrap(), connection)
}

fn repository() -> Arc<Mutex<Repository>> {
    Arc::new(Mutex::new(Repository { commit: "a".repeat(40), ..Default::default() }))
}

fn stage(connection: &Connection, document: &[u8], parent: Option<String>) -> crate::StagedSave {
    PendingSave::stage(
        connection,
        "Robot",
        "parts/bracket",
        parent,
        Snapshot::prepare(document.to_vec(), BTreeMap::from([("blobs/source.step".into(), vec![42; 1000])])).unwrap(),
    )
    .unwrap()
}

fn recover(save: &crate::StagedSave) -> Result<Snapshot> {
    save.record.recover(|path| Ok(save.objects[path].clone()))
}

fn run_test(future: impl std::future::Future<Output = ()>) {
    let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
    tokio::task::LocalSet::new().block_on(&runtime, future);
}

#[test]
fn publication_reuses_assets_preserves_other_files_and_retries_without_a_second_revision() {
    run_test(async {
        let repository = repository();
        let (api, connection) = adapter(repository.clone());
        let save = stage(&connection, b"{}", None);
        {
            let mut repo = repository.lock().unwrap();
            repo.files.insert("docs/README.md".into(), b"Unrelated documentation".to_vec());
            for (path, bytes) in &recover(&save).unwrap().objects {
                repo.files.insert(format!("Robot/{path}"), bytes.clone());
            }
        }
        let published = api.publish(&save.record, || recover(&save), |_| Ok(())).await.unwrap();
        assert!(!published.conflict);
        let reopened = api.open(&published.destination, |_| Ok(())).await.unwrap();
        assert_eq!(reopened.document, b"{}");
        assert_eq!(reopened.objects, recover(&save).unwrap().objects);
        let again = api.publish(&save.record, || recover(&save), |_| Ok(())).await.unwrap();
        assert_eq!(again.commit, published.commit);
        let repo = repository.lock().unwrap();
        assert_eq!(repo.commits, 1);
        assert_eq!(repo.files["docs/README.md"], b"Unrelated documentation");
        let changes = &repo.requests.iter().find(|(method, path, _)| method == "POST" && path.ends_with("/git/trees")).unwrap().2["tree"];
        assert!(changes.as_array().unwrap().iter().all(|entry| !entry["path"].as_str().unwrap().contains("/objects/")));
    });
}

#[test]
fn future_formats_and_symlinks_are_rejected_before_any_remote_write() {
    run_test(async {
        for symbolic_link in [false, true] {
            let repository = repository();
            let (api, connection) = adapter(repository.clone());
            let save = stage(&connection, b"{}", None);
            {
                let mut repo = repository.lock().unwrap();
                let marker = "Robot/solidcraft-project.json".to_string();
                repo.files.insert(marker.clone(), br#"{"format":"solidcraft-cloud/2"}"#.to_vec());
                if symbolic_link {
                    repo.modes.insert(marker, "120000".into());
                }
            }
            let result = api.publish(&save.record, || recover(&save), |_| Ok(())).await;
            if symbolic_link {
                assert!(matches!(result, Err(Error::Invalid(_))));
            } else {
                assert!(matches!(result, Err(Error::UpgradeRequired(_))));
            }
            assert!(repository.lock().unwrap().requests.iter().all(|(method, _, _)| method == "GET"));
        }
    });
}

#[test]
fn a_ref_race_preserves_the_external_design_and_publishes_a_conflict_copy() {
    run_test(async {
        let repository = repository();
        let (api, connection) = adapter(repository.clone());
        let first = stage(&connection, b"{}", None);
        api.publish(&first.record, || recover(&first), |_| Ok(())).await.unwrap();
        let save = stage(&connection, br#"{"edit":true}"#, Some(first.record.id));
        repository.lock().unwrap().compete_once = true;
        let published = api.publish(&save.record, || recover(&save), |_| Ok(())).await.unwrap();
        assert!(published.conflict);
        let repo = repository.lock().unwrap();
        let original = Manifest::read(&repo.files["Robot/parts/bracket/manifest.json"]).unwrap();
        assert_eq!(crate::revision(&original), Some("external"));
        assert_eq!(repo.files[&format!("{}/document.json", published.destination.design_path().unwrap())], br#"{"edit":true}"#);
    });
}

#[test]
fn a_corrupt_existing_object_is_never_reused_as_a_successful_save() {
    run_test(async {
        let repository = repository();
        let (api, connection) = adapter(repository.clone());
        let save = stage(&connection, b"{}", None);
        for path in recover(&save).unwrap().objects.keys() {
            repository.lock().unwrap().files.insert(format!("Robot/{path}"), b"corrupt object".to_vec());
        }
        assert!(matches!(api.publish(&save.record, || recover(&save), |_| Ok(())).await, Err(Error::Invalid(_))));
        assert!(repository.lock().unwrap().requests.iter().all(|(_, path, _)| !path.ends_with("/git/refs/heads/main")));
    });
}

#[test]
fn guided_setup_discovers_the_account_and_default_repository_only_without_an_override() {
    run_test(async {
        let repository = repository();
        repository.lock().unwrap().automatic = true;
        let (api, _) = adapter(repository.clone());
        let mut automatic = crate::setup::github_connection("").unwrap();
        automatic.credential = "local-credential-reference".into();
        let prepared = prepare(api.client.clone(), automatic).await.unwrap();
        assert_eq!(prepared.repository, "owner/solidcraft-designs");
        assert_eq!(prepared.id, crate::setup::github_connection("owner/solidcraft-designs").unwrap().id);
        assert_eq!(prepared.credential, "local-credential-reference");
        prepare(api.client.clone(), prepared).await.unwrap();
        assert_eq!(repository.lock().unwrap().requests.iter().filter(|(_, path, _)| path == "/user").count(), 1);
        repository.lock().unwrap().denied = true;
        let before = repository.lock().unwrap().requests.len();
        assert!(prepare(api.client.clone(), crate::setup::github_connection("").unwrap()).await.is_err());
        assert_eq!(repository.lock().unwrap().requests.len(), before + 1, "authentication failure cannot guess another account");
    });
}

#[test]
fn guided_setup_initializes_empty_repositories_once_and_preserves_existing_content() {
    tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(tokio::task::LocalSet::new().run_until(async {
        for empty in [false, true] {
            let repository = repository();
            repository.lock().unwrap().empty = empty;
            let (adapter, connection) = adapter(repository.clone());
            let prepared = prepare(adapter.client.clone(), connection).await.unwrap();
            assert_eq!(prepared.remote, "https://github.com/owner/designs.git");
            assert_eq!(prepared.branch, "main");
            prepare(adapter.client.clone(), prepared).await.unwrap();
            let repo = repository.lock().unwrap();
            assert_eq!(repo.requests.iter().filter(|(method, _, _)| method == "PUT").count(), usize::from(empty));
            assert_eq!(repo.requests.iter().filter(|(method, _, _)| method == "PATCH").count(), 2);
        }
    }));
}

#[test]
fn guided_setup_reports_denied_or_readonly_repositories_without_writes() {
    tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(tokio::task::LocalSet::new().run_until(async {
        for denied in [false, true] {
            let repository = repository();
            {
                let mut repo = repository.lock().unwrap();
                repo.denied = denied;
                repo.readonly = !denied;
            }
            let (adapter, connection) = adapter(repository.clone());
            let message = prepare(adapter.client.clone(), connection).await.err().unwrap().to_string();
            assert!(message.contains(if denied { "access denied" } else { "does not allow pushes" }));
            assert!(repository.lock().unwrap().requests.iter().all(|(method, _, _)| method == "GET"));
        }
    }));
}

#[test]
fn setup_checks_token_write_permission_even_when_account_can_push() {
    tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(tokio::task::LocalSet::new().run_until(async {
        let repository = repository();
        repository.lock().unwrap().write_denied = true;
        let (adapter, connection) = adapter(repository.clone());
        assert!(prepare(adapter.client.clone(), connection).await.err().unwrap().to_string().contains("access denied"));
        let state = repository.lock().unwrap();
        assert_eq!(state.commits, 0);
        let (_, _, probe) = state.requests.iter().find(|(method, _, _)| method == "PATCH").unwrap();
        assert_eq!(probe["force"], false);
        assert_eq!(probe["sha"], state.commit);
    }));
}
