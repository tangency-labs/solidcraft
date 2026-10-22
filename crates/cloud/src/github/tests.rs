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
        if method == "GET" && path.contains("/git/ref/") {
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
