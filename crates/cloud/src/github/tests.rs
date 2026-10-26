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
    not_found: bool,
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
            if self.not_found {
                return (404, json!({"message":"not found","documentation_url":URL}));
            }
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
        if method == "GET" && path.ends_with("/commits") {
            let mut commit = self.commit(&self.commit);
            commit["comment_count"] = json!(0);
            return (
                200,
                json!([{"sha":self.commit,"node_id":"commit","url":URL,"html_url":URL,"comments_url":URL,"commit":commit,"author":null,"committer":null,"parents":[]} ]),
            );
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
fn repository_existence_check_is_one_read_and_handles_private_or_missing_repositories() {
    run_test(async {
        for (missing, denied) in [(false, false), (true, false), (false, true)] {
            let state = repository();
            {
                let mut state = state.lock().unwrap();
                state.empty = true;
                state.not_found = missing;
                state.denied = denied;
            }
            let (api, _) = adapter(state.clone());
            let result = super::exists(api.client, "owner/designs").await;
            if denied {
                assert!(result.is_err());
            } else {
                assert_eq!(result.unwrap(), !missing);
            }
            let state = state.lock().unwrap();
            assert_eq!(state.requests.len(), 1);
            assert_eq!(state.requests[0].0, "GET");
            assert_eq!(state.requests[0].1, "/repos/owner/designs");
            assert!(!state.initialized, "a preview check must not initialize or write the repository");
        }
    });
}

#[test]
fn management_publication_history_and_retry_use_atomic_non_forced_commits() {
    use crate::management::Change;
    run_test(async {
        let repository = repository();
        let (api, connection) = adapter(repository.clone());
        let first = stage(&connection, b"{}", None);
        let source = first.record.destination.clone();
        let saved = api.publish(&first.record, || recover(&first), |_| Ok(())).await.unwrap();
        let original = api.open_at(&source, &saved.commit, |_| Ok(())).await.unwrap();
        let history = api.history(&source, |_| Ok(())).await.unwrap();
        assert_eq!(history.revisions.len(), 1);
        assert_eq!(history.revisions[0].commit, saved.commit);
        let change = Change::Rename { file: "parts/renamed bracket".into() };
        let mut snapshot = api.open(&source, |_| Ok(())).await.unwrap();
        snapshot.manifest.set_change(&change).unwrap();
        let rename = PendingSave::stage(&connection, &source.project, &source.file, Some(saved.revision), snapshot).unwrap();
        let renamed = api.publish(&rename.record, || recover(&rename), |_| Ok(())).await.unwrap();
        assert!(!renamed.conflict);
        assert_eq!(api.list().await.unwrap(), vec![renamed.destination.clone()]);
        assert!(api.list_deleted().await.unwrap().is_empty());
        let commits = repository.lock().unwrap().commits;
        let retry = api.publish(&rename.record, || recover(&rename), |_| Ok(())).await.unwrap();
        assert_eq!(retry.revision, renamed.revision);
        assert_eq!(repository.lock().unwrap().commits, commits);
        let mut snapshot = api.open(&renamed.destination, |_| Ok(())).await.unwrap();
        snapshot.manifest.set_change(&Change::Delete).unwrap();
        let delete = PendingSave::stage(&connection, &source.project, &renamed.destination.file, Some(renamed.revision), snapshot).unwrap();
        let deleted = api.publish(&delete.record, || recover(&delete), |_| Ok(())).await.unwrap();
        assert!(api.list().await.unwrap().is_empty());
        assert_eq!(api.list_deleted().await.unwrap(), vec![renamed.destination.clone()]);
        let mut snapshot = original;
        snapshot.manifest.set_change(&Change::Restore { commit: saved.commit, file: source.file.clone() }).unwrap();
        let restore = PendingSave::stage(&connection, &source.project, &renamed.destination.file, Some(deleted.revision), snapshot).unwrap();
        api.publish(&restore.record, || recover(&restore), |_| Ok(())).await.unwrap();
        assert!(api.list_deleted().await.unwrap().is_empty());
        assert_eq!(api.list().await.unwrap(), vec![renamed.destination]);
    });
}

#[test]
fn rename_never_overwrites_an_existing_document_without_a_manifest() {
    use crate::management::Change;
    run_test(async {
        let repository = repository();
        let (api, connection) = adapter(repository.clone());
        let first = stage(&connection, b"{}", None);
        let source = first.record.destination.clone();
        let saved = api.publish(&first.record, || recover(&first), |_| Ok(())).await.unwrap();
        repository.lock().unwrap().files.insert("Robot/parts/occupied/document.json".into(), b"{}".to_vec());
        let mut snapshot = api.open(&source, |_| Ok(())).await.unwrap();
        snapshot.manifest.set_change(&Change::Rename { file: "parts/occupied".into() }).unwrap();
        let rename = PendingSave::stage(&connection, &source.project, &source.file, Some(saved.revision), snapshot).unwrap();
        let commits = repository.lock().unwrap().commits;
        assert!(matches!(api.publish(&rename.record, || recover(&rename), |_| Ok(())).await, Err(Error::Conflict)));
        assert_eq!(repository.lock().unwrap().commits, commits);
    });
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
fn offline_children_update_the_parents_conflict_copy_and_retries_do_not_commit_again() {
    run_test(async {
        let repository = repository();
        let (api, connection) = adapter(repository.clone());
        let original = stage(&connection, b"external", None);
        let original_publication = api.publish(&original.record, || recover(&original), |_| Ok(())).await.unwrap();
        let parent = stage(&connection, b"offline first", None);
        let published = api.publish(&parent.record, || recover(&parent), |_| Ok(())).await.unwrap();
        assert!(published.conflict);
        let child = stage(&connection, b"offline next", Some(parent.record.id.clone()));
        let child_publication =
            api.publish_with_parent(&child.record, || std::future::ready(recover(&child)), |_| Ok(()), Some(&published)).await.unwrap();
        child_publication.validate(&child.record, Some(&published)).unwrap();
        assert_eq!(child_publication.destination, published.destination);
        assert!(child_publication.conflict);
        assert_eq!(api.open(&original_publication.destination, |_| Ok(())).await.unwrap().document, b"external");
        assert_eq!(api.open(&published.destination, |_| Ok(())).await.unwrap().document, b"offline next");
        let commits = repository.lock().unwrap().commits;
        let retry = api.publish_with_parent(&child.record, || std::future::ready(recover(&child)), |_| Ok(()), Some(&published)).await.unwrap();
        assert_eq!(retry.destination, child_publication.destination);
        assert_eq!(repository.lock().unwrap().commits, commits);
        let mut wrong = published;
        wrong.destination.project = "Other".into();
        assert!(matches!(
            api.publish_with_parent(&child.record, || std::future::ready(recover(&child)), |_| Ok(()), Some(&wrong)).await,
            Err(Error::Invalid(_))
        ));
    });
}

#[test]
fn listing_observes_outdated_cached_heads_without_downloading_source_assets() {
    run_test(async {
        let repository = repository();
        let (api, connection) = adapter(repository.clone());
        let initial = stage(&connection, b"first", None);
        let published = api.publish(&initial.record, || recover(&initial), |_| Ok(())).await.unwrap();
        let cached = crate::CachedEntry::new(initial.record.clone(), published.destination.clone(), Some(published.revision), true).unwrap();
        let mut catalog = crate::stored_designs(vec![cached], vec![]).unwrap();
        assert_eq!(api.list_with_status(&mut catalog).await.unwrap(), vec![published.destination]);
        assert!(!catalog[0].outdated);
        let changed = stage(&connection, b"second", Some(initial.record.id));
        api.publish(&changed.record, || recover(&changed), |_| Ok(())).await.unwrap();
        repository.lock().unwrap().requests.clear();
        api.list_with_status(&mut catalog).await.unwrap();
        assert!(catalog[0].outdated);
        assert_eq!(catalog[0].phase, crate::jobs::Phase::Ready);
        let repo = repository.lock().unwrap();
        let source_hashes: std::collections::BTreeSet<_> = changed
            .record
            .manifest
            .assets
            .values()
            .flat_map(|a| a.chunks.iter())
            .map(|c| blob_hash(&changed.objects[&c.path().unwrap()], 40).unwrap())
            .collect();
        assert!(repo.requests.iter().all(|(_, path, _)| !source_hashes.iter().any(|hash| path.ends_with(hash))), "listing fetches metadata only");
    });
}

#[test]
fn previews_fetch_only_the_image_and_optional_corruption_does_not_block_opening() {
    run_test(async {
        let repository = repository();
        let (api, connection) = adapter(repository.clone());
        let mut snapshot = Snapshot::prepare(b"{}".to_vec(), BTreeMap::from([("blobs/source.step".into(), vec![42; 512 << 10])])).unwrap();
        let png = include_bytes!("../../../../assets/samples/exports/0.png").to_vec();
        crate::attach_thumbnail(&mut snapshot, png.clone()).unwrap();
        let preview = crate::Thumbnail::read(&snapshot.manifest).unwrap().unwrap().paths().unwrap().pop().unwrap();
        let source = snapshot.manifest.assets.values().next().unwrap().chunks.first().unwrap().path().unwrap();
        let save = PendingSave::stage(&connection, "Robot", "parts/bracket", None, snapshot).unwrap();
        let published = api.publish(&save.record, || recover(&save), |_| Ok(())).await.unwrap();
        let bytes = repository.lock().unwrap().files.remove(&format!("Robot/{source}")).unwrap();
        repository.lock().unwrap().requests.clear();
        assert_eq!(api.thumbnail(&published.destination, |_| Ok(())).await.unwrap(), Some(png));
        let requested = repository.lock().unwrap().requests.clone();
        assert_eq!(requested.iter().filter(|(method, path, _)| method == "GET" && path.contains("/git/blobs/")).count(), 4);
        {
            let mut repo = repository.lock().unwrap();
            repo.files.insert(format!("Robot/{source}"), bytes);
            repo.files.insert(format!("Robot/{preview}"), b"damaged optional image".to_vec());
        }
        let reopened = api.open(&published.destination, |_| Ok(())).await.unwrap();
        assert_eq!(reopened.document, b"{}");
        assert!(crate::Thumbnail::png(&reopened.manifest, |p| reopened.objects.get(p).cloned().ok_or_else(|| storage("missing"))).unwrap().is_none());
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
