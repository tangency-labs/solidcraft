use std::io::Write;
use std::sync::Arc;

use ed25519_dalek::{Signer, SigningKey};
use serde_json::{Value, json};

use super::index::{self, Refusal, Tier};
use super::link::{self, Kind, Link, Target};
use super::*;

const NOW: i64 = 1_795_000_000; // 2026-11-17
const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn signing() -> SigningKey {
    SigningKey::from_bytes(&[7u8; 32])
}

fn test_key() -> (String, String) {
    let public = signing().verifying_key().to_bytes();
    (index::key_id(&public), public.iter().map(|b| std::format!("{b:02x}")).collect())
}

fn sign(body: &[u8]) -> String {
    signing().sign(body).to_bytes().iter().map(|b| std::format!("{b:02x}")).collect()
}

fn file(role: &str, url: &str) -> Value {
    json!({"name": "f", "role": role, "size": 48211, "sha256": SHA, "url": url})
}

fn index_json(key_id: &str) -> Value {
    json!({
        "format": "solidcraft-workshop-index/1",
        "issuer": "https://workshop.solidcraft.io",
        "key_id": key_id,
        "sequence": 42,
        "issued_at": "2026-11-16T03:00:00.000Z",
        "expires_at": "2026-12-03T03:00:00.000Z",
        "items": [
            {
                "id": "gears", "kind": "plugin", "owner": "maker", "slug": "gears", "name": "Gears",
                "summary": "Spur and helical gear generator.", "tier": "community",
                "author": {"handle": "maker", "name": "A Maker"}, "licence": "MIT",
                "versions": [
                    {"version": "2.0.0", "min_app_version": "99.0.0", "permissions": ["commands:sketch"],
                     "files": [file("package", "https://workshop.solidcraft.io/api/files/2/download")]},
                    {"version": "1.2.0", "min_app_version": "0.40.0", "permissions": ["commands:sketch", "files:read"],
                     "files": [file("package", "https://workshop.solidcraft.io/api/files/1/download"), file("image", "https://workshop.solidcraft.io/api/files/9")]}
                ]
            },
            {
                "id": "k3x9", "kind": "model", "owner": "bherbruck", "slug": "wall-mount-shelf-bracket", "name": "Wall mount shelf bracket",
                "tier": "unverified", "author": {"handle": "bherbruck"}, "licence": "CC-BY-4.0",
                "versions": [
                    {"version": "1.2.0", "files": [file("design", "https://workshop.solidcraft.io/api/files/3/download")]},
                    {"version": "1.1.0", "files": [file("design", "https://workshop.solidcraft.io/api/files/4/download")]}
                ]
            },
            {
                "id": "tangency-labs.fasteners", "kind": "library", "owner": "tangency-labs", "slug": "fasteners", "name": "Fasteners",
                "tier": "official", "author": {"handle": "tangency-labs", "name": "Tangency Labs"}, "licence": "MIT",
                "versions": [{"version": "1.2.0", "files": [file("library", "https://github.com/tangency-labs/fasteners/releases/download/v1.2.0/fasteners-1.2.0.zip")]}]
            },
            {
                "id": "evil", "kind": "model", "owner": "x", "slug": "evil", "name": "Elsewhere", "tier": "unverified",
                "versions": [{"version": "1.0.0", "files": [file("design", "https://example.com/evil.sc")]}]
            }
        ],
        "delisted": [{"id": "old-thing", "kind": "model", "reason": "Author asked"}],
        "revoked": [
            {"id": "k3x9", "version": "1.2.0", "severity": "block", "reason": "Malware", "sha256": []},
            {"id": "gears", "version": "1.2.0", "severity": "warn", "reason": "Slow on big sketches", "sha256": []}
        ]
    })
}

fn signed(value: &Value) -> Signed {
    let (key_id, _) = test_key();
    let body = serde_json::to_vec(value).unwrap();
    Signed { signature: Some(sign(&body)), key_id: Some(key_id), body }
}

fn keys() -> Vec<(String, String)> {
    vec![test_key()]
}

fn run(answer: Signed, link: &str, seen: Option<&Path>) -> Result<index::Resolved, Refusal> {
    let keys = keys();
    let keys: Vec<(&str, &str)> = keys.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let link = link::parse(link).expect("a valid link");
    lookup(
        &keys,
        &|url: &str| {
            assert_eq!(url, index::INDEX_URL);
            Ok(answer.clone())
        },
        &link,
        seen,
        NOW,
        &semver::Version::parse("0.48.0-nightly.20261126.1").unwrap(),
    )
}

#[test]
fn good_links_parse() {
    let v = |s: &str| Some(semver::Version::parse(s).unwrap());
    let address = |kind, owner: &str, slug: &str, version| Link { kind, target: Target::Address { owner: owner.into(), slug: slug.into() }, version };
    assert_eq!(
        link::parse("solidcraft://workshop/model/bherbruck/wall-mount-shelf-bracket@1.2.0"),
        Some(address(Kind::Model, "bherbruck", "wall-mount-shelf-bracket", v("1.2.0")))
    );
    assert_eq!(link::parse("solidcraft://workshop/plugin/maker/gears"), Some(address(Kind::Plugin, "maker", "gears", None)));
    assert_eq!(link::parse("solidcraft://workshop/plugin/maker/gears/"), Some(address(Kind::Plugin, "maker", "gears", None)));
    assert_eq!(
        link::parse("solidcraft://workshop/part/tangency-labs/fasteners@1.2.0"),
        Some(address(Kind::Part, "tangency-labs", "fasteners", v("1.2.0")))
    );
    // The first format: a stable id, `library` for parts libraries.
    assert_eq!(
        link::parse("solidcraft://workshop/plugin/gears@2.0.1"),
        Some(Link { kind: Kind::Plugin, target: Target::Id("gears".into()), version: v("2.0.1") })
    );
    assert_eq!(
        link::parse("solidcraft://workshop/library/tangency-labs.fasteners"),
        Some(Link { kind: Kind::Part, target: Target::Id("tangency-labs.fasteners".into()), version: None })
    );
}

#[test]
fn bad_links_are_refused() {
    for bad in [
        "",
        "solidcraft://",
        "solidcraft://workshop/",
        "solidcraft://workshop/model",
        "solidcraft://workshop/theme/maker/gears",
        "solidcraft://other/model/maker/gears",
        "https://workshop.solidcraft.io/model/maker/gears",
        "file:///etc/passwd",
        "solidcraft://workshop/model/maker/gears/extra",
        "solidcraft://workshop/model/maker//gears",
        "solidcraft://workshop/model/../gears",
        "solidcraft://workshop/model/maker/..",
        "solidcraft://workshop/model/maker/Gears",
        "solidcraft://workshop/model/maker/-gears",
        "solidcraft://workshop/model/maker/gears-",
        "solidcraft://workshop/model/maker/gears@1.2",
        "solidcraft://workshop/model/maker/gears@1.2.3.4",
        "solidcraft://workshop/model/maker/gears@1.2.3-rc.1",
        "solidcraft://workshop/model/maker/gears@v1.2.3",
        "solidcraft://workshop/model/maker/gears@",
        "solidcraft://workshop/model/maker/gears@1.2.3@1.2.3",
        "solidcraft://workshop/model/maker/gears?url=https://evil.example",
        "solidcraft://workshop/model/maker/gears#x",
        "solidcraft://workshop/model/maker/ge%2Fars",
        "solidcraft://workshop/model/maker/gears//",
        "solidcraft://workshop/model/maker/gé",
        "solidcraft://workshop/model/maker/ge ars",
        "solidcraft://workshop/model/maker/gears\n",
        "solidcraft://workshop/model\\maker\\gears",
        "solidcraft://user@workshop/model/maker/gears",
        "SOLIDCRAFT://workshop/model/maker/gears",
    ] {
        assert_eq!(link::parse(bad), None, "{bad:?}");
    }
    let long = std::format!("solidcraft://workshop/model/maker/{}", "a".repeat(129));
    assert_eq!(link::parse(&long), None);
    let huge = std::format!("solidcraft://workshop/model/{}", "a/".repeat(400));
    assert_eq!(link::parse(&huge), None);
    assert!(link::looks_like_link("solidcraft://workshop/x"));
    assert!(!link::looks_like_link("/home/me/solidcraft://workshop/part.sc"));
}

#[test]
fn the_built_in_key_id_is_its_hash() {
    for (id, key) in index::KEYS {
        let bytes: Vec<u8> = (0..32).map(|i| u8::from_str_radix(&key[i * 2..i * 2 + 2], 16).unwrap()).collect();
        assert_eq!(index::key_id(&bytes.try_into().unwrap()), *id);
    }
    assert_eq!(index::KEYS.first().map(|k| k.0), Some("5247b5af3f50dbc4"));
}

#[test]
fn a_good_index_resolves_the_newest_version_that_runs() {
    let item = run(signed(&index_json(&test_key().0)), "solidcraft://workshop/plugin/maker/gears", None).unwrap();
    // 2.0.0 needs SolidCraft 99; 1.2.0 runs on this 0.48 nightly.
    assert_eq!(item.version, semver::Version::new(1, 2, 0));
    assert_eq!(item.tier, Tier::Community);
    assert_eq!(item.author, "A Maker (@maker)");
    assert_eq!(item.permissions, ["commands:sketch", "files:read"]);
    assert_eq!(item.file.url, "https://workshop.solidcraft.io/api/files/1/download");
    assert_eq!(item.warning.as_deref(), Some("Slow on big sketches"));
    // The old id link finds the same item.
    let by_id = run(signed(&index_json(&test_key().0)), "solidcraft://workshop/plugin/gears@1.2.0", None).unwrap();
    assert_eq!(by_id, item);
    // A library's file may come from its GitHub release.
    let part = run(signed(&index_json(&test_key().0)), "solidcraft://workshop/part/tangency-labs/fasteners@1.2.0", None).unwrap();
    assert_eq!((part.kind, part.id.as_str(), part.tier), (Kind::Part, "tangency-labs.fasteners", Tier::Official));
}

#[test]
fn a_failed_signature_is_refused() {
    let good = signed(&index_json(&test_key().0));
    // One byte changed after signing.
    let mut tampered = good.clone();
    let at = tampered.body.iter().position(|b| *b == b'4').unwrap();
    tampered.body[at] = b'5';
    assert_eq!(run(tampered, "solidcraft://workshop/plugin/maker/gears", None), Err(Refusal::Untrusted));
    // No signature header.
    assert_eq!(run(Signed { signature: None, ..good.clone() }, "solidcraft://workshop/plugin/maker/gears", None), Err(Refusal::Untrusted));
    // A key SolidCraft doesn't carry.
    assert_eq!(
        run(Signed { key_id: Some("0000000000000000".into()), ..good.clone() }, "solidcraft://workshop/plugin/maker/gears", None),
        Err(Refusal::Untrusted)
    );
    // Not hex, or the wrong length.
    assert_eq!(
        run(Signed { signature: Some("zz".repeat(64)), ..good.clone() }, "solidcraft://workshop/plugin/maker/gears", None),
        Err(Refusal::Untrusted)
    );
    // Signed, but the body names another key.
    assert_eq!(run(signed(&index_json("5247b5af3f50dbc4")), "solidcraft://workshop/plugin/maker/gears", None), Err(Refusal::Untrusted));
    // The production key does not verify a test signature.
    assert_eq!(index::verify(&good.body, good.signature.as_deref(), Some("5247b5af3f50dbc4")).err(), Some(Refusal::Untrusted));
    // Unreachable is said, not guessed around.
    let keys = keys();
    let keys: Vec<(&str, &str)> = keys.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let link = link::parse("solidcraft://workshop/plugin/maker/gears").unwrap();
    let offline = lookup(&keys, &|_| Err("offline".into()), &link, None, NOW, &semver::Version::new(0, 48, 0));
    assert_eq!(offline, Err(Refusal::Unreachable("offline".into())));
}

#[test]
fn revoked_and_delisted_items_are_refused() {
    let index = signed(&index_json(&test_key().0));
    assert_eq!(
        run(index.clone(), "solidcraft://workshop/model/bherbruck/wall-mount-shelf-bracket@1.2.0", None),
        Err(Refusal::Revoked(Some("Malware".into())))
    );
    // Without a version, the newest one that isn't revoked.
    let newest = run(index.clone(), "solidcraft://workshop/model/bherbruck/wall-mount-shelf-bracket", None).unwrap();
    assert_eq!(newest.version, semver::Version::new(1, 1, 0));
    assert_eq!(run(index.clone(), "solidcraft://workshop/model/old-thing", None), Err(Refusal::Delisted(Some("Author asked".into()))));
    // Every version revoked, and a file whose content is revoked under any item.
    let mut all = index_json(&test_key().0);
    all["revoked"] = json!([{"id": "gears", "version": null, "severity": "block", "reason": "Malware", "sha256": []}]);
    assert_eq!(run(signed(&all), "solidcraft://workshop/plugin/maker/gears", None), Err(Refusal::Revoked(Some("Malware".into()))));
    let mut by_hash = index_json(&test_key().0);
    by_hash["revoked"] = json!([{"id": "other", "version": "1.0.0", "severity": "block", "sha256": [SHA]}]);
    assert_eq!(run(signed(&by_hash), "solidcraft://workshop/plugin/maker/gears", None), Err(Refusal::Revoked(None)));
    // Unknown, missing and too-new versions, and a file from another host.
    assert!(matches!(run(index.clone(), "solidcraft://workshop/plugin/maker/nope", None), Err(Refusal::NotListed(_))));
    assert_eq!(run(index.clone(), "solidcraft://workshop/plugin/maker/gears@1.0.0", None), Err(Refusal::NoSuchVersion("1.0.0".into())));
    assert_eq!(run(index.clone(), "solidcraft://workshop/plugin/maker/gears@2.0.0", None), Err(Refusal::NeedsNewerApp("99.0.0".into())));
    assert!(matches!(run(index.clone(), "solidcraft://workshop/model/x/evil", None), Err(Refusal::BadItem(_))));
    // The kind is part of the address.
    assert!(matches!(run(index, "solidcraft://workshop/model/maker/gears", None), Err(Refusal::NotListed(_))));
}

#[test]
fn a_block_matches_the_version_as_semver_reads_it() {
    // `v1.2.0` and `1.2.0+build.7` name the version the link asks for as `1.2.0`.
    for listed in ["v1.2.0", "1.2.0+build.7", " 1.2.0 "] {
        let mut index = index_json(&test_key().0);
        index["revoked"] = json!([{"id": "gears", "version": listed, "severity": "block", "reason": "Malware", "sha256": []}]);
        assert_eq!(
            run(signed(&index), "solidcraft://workshop/plugin/maker/gears@1.2.0", None),
            Err(Refusal::Revoked(Some("Malware".into()))),
            "{listed}"
        );
        // Without a version, the blocked one is skipped too: 2.0.0 needs a newer app, so nothing runs.
        assert_eq!(run(signed(&index), "solidcraft://workshop/plugin/maker/gears", None), Err(Refusal::NeedsNewerApp("99.0.0".into())), "{listed}");
    }
    // Another version stays usable, and a record whose version doesn't parse blocks every version.
    let mut other = index_json(&test_key().0);
    other["revoked"] = json!([{"id": "k3x9", "version": "1.2.1", "severity": "block", "sha256": []}]);
    assert_eq!(
        run(signed(&other), "solidcraft://workshop/model/bherbruck/wall-mount-shelf-bracket@1.2.0", None).unwrap().version,
        semver::Version::new(1, 2, 0)
    );
    let mut garbled = index_json(&test_key().0);
    garbled["revoked"] = json!([{"id": "k3x9", "version": "1.2.x", "severity": "block", "reason": "Malware", "sha256": []}]);
    assert_eq!(
        run(signed(&garbled), "solidcraft://workshop/model/bherbruck/wall-mount-shelf-bracket@1.1.0", None),
        Err(Refusal::Revoked(Some("Malware".into())))
    );
}

#[test]
fn an_older_or_expired_index_is_refused() {
    let dir = std::env::temp_dir().join(std::format!("sc-workshop-seq-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let seen = dir.join("index-sequence");
    let link = "solidcraft://workshop/plugin/maker/gears";
    assert!(run(signed(&index_json(&test_key().0)), link, Some(&seen)).is_ok());
    assert_eq!(std::fs::read_to_string(&seen).unwrap(), "42");
    let mut older = index_json(&test_key().0);
    older["sequence"] = json!(41);
    assert_eq!(run(signed(&older), link, Some(&seen)), Err(Refusal::Replayed { seen: 42, got: 41 }));
    // The same sequence re-issued is fine.
    assert!(run(signed(&index_json(&test_key().0)), link, Some(&seen)).is_ok());
    let mut expired = index_json(&test_key().0);
    expired["sequence"] = json!(43);
    expired["expires_at"] = json!("2026-11-01T00:00:00Z");
    assert_eq!(run(signed(&expired), link, Some(&seen)), Err(Refusal::Expired));
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(index::unix_seconds("1970-01-01T00:00:00Z"), Some(0));
    assert_eq!(index::unix_seconds("2026-12-03T03:00:00.000Z"), Some(1_796_266_800));
    assert_eq!(index::unix_seconds("2026-13-03T03:00:00Z"), None);
    assert_eq!(index::unix_seconds("2026-12-03 03:00:00"), None);
}

#[test]
fn the_question_shows_what_is_installed() {
    crate::i18n::install();
    let _language = solidcraft_ui_text::scope("en");
    let plugin = run(signed(&index_json(&test_key().0)), "solidcraft://workshop/plugin/maker/gears", None).unwrap();
    let q = question(&plugin);
    assert_eq!(q.title, "Install plugin from the Workshop");
    assert_eq!(q.ok, "Install");
    assert_eq!(q.heading, "Gears 1.2.0");
    let rows: Vec<(&str, &str)> = q.rows.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    assert_eq!(
        rows,
        [("Kind", "Plugin"), ("Author", "A Maker (@maker)"), ("Tier", "Community"), ("Version", "1.2.0"), ("Size", "47 KB"), ("License", "MIT")]
    );
    assert_eq!(q.permissions, ["commands:sketch", "files:read"]);
    assert_eq!(q.tone, Tone::Warning, "a warn revocation is shown");
    assert!(q.warnings.iter().any(|w| w.contains("Slow on big sketches")), "{:?}", q.warnings);
    assert!(q.next.contains("permissions"), "{}", q.next);

    let model = run(signed(&index_json(&test_key().0)), "solidcraft://workshop/model/bherbruck/wall-mount-shelf-bracket", None).unwrap();
    let q = question(&model);
    assert_eq!((q.title.as_str(), q.ok.as_str()), ("Open from the Workshop", "Open"));
    assert!(q.permissions.is_empty());
    assert!(q.rows.iter().any(|(label, value)| label == "Tier" && value == "Unverified"));
    assert!(q.rows.iter().any(|(label, value)| label == "Author" && value == "@bherbruck"));
    assert_eq!(q.tone, Tone::Warning, "an unverified item is shown as such");

    let part = run(signed(&index_json(&test_key().0)), "solidcraft://workshop/part/tangency-labs/fasteners", None).unwrap();
    let q = question(&part);
    assert_eq!(q.title, "Install parts library from the Workshop");
    assert!(q.rows.iter().any(|(label, value)| label == "Tier" && value == "Official"));
    assert_eq!(q.tone, Tone::Info);
    assert!(q.warnings.is_empty());

    // A plugin that asks for nothing says so.
    let mut none = plugin.clone();
    none.permissions.clear();
    assert_eq!(question(&none).permissions, ["No permissions"]);
    // Every refusal has words.
    for refusal in [
        Refusal::BadLink,
        Refusal::Untrusted,
        Refusal::Expired,
        Refusal::Checksum,
        Refusal::Revoked(None),
        Refusal::Delisted(Some("Author asked".into())),
    ] {
        let text = refusal_text(&refusal);
        assert!(!text.is_empty() && !text.starts_with("workshop."), "{text}");
    }
    assert!(refusal_text(&Refusal::Revoked(Some("Malware".into()))).contains("Malware"));
}

#[test]
fn a_download_that_does_not_match_is_dropped() {
    let item = run(signed(&index_json(&test_key().0)), "solidcraft://workshop/plugin/maker/gears", None).unwrap();
    let dir = download_dir().unwrap();
    let download: crate::update::install::HttpDownload = Arc::new(|_url: &str, to: &Path, _progress: &dyn Fn(u64, Option<u64>) -> bool| {
        std::fs::write(to, b"not the package").unwrap();
        Ok(())
    });
    assert_eq!(fetch(&download, &item, &dir).err(), Some(Refusal::Checksum));
    assert!(!dir.join("gears-1.2.0.zip").exists(), "nothing is kept");
    // Matching bytes are kept under a name made from the item, not the index's file name.
    let bytes = b"the package";
    let mut good = item.clone();
    good.file.sha256 = <sha2::Sha256 as sha2::Digest>::digest(bytes).iter().map(|b| std::format!("{b:02x}")).collect();
    let download: crate::update::install::HttpDownload = Arc::new(move |_url: &str, to: &Path, _: &dyn Fn(u64, Option<u64>) -> bool| {
        std::fs::write(to, bytes).unwrap();
        Ok(())
    });
    let (path, mut file) = fetch(&download, &good, &dir).unwrap();
    assert_eq!(path, dir.join("gears-1.2.0.zip"));
    // The handle the check read, rewound for whoever uses it.
    let mut read = Vec::new();
    std::io::Read::read_to_end(&mut file, &mut read).unwrap();
    assert_eq!(read, bytes);
    let checked = seen(&file.metadata().unwrap());
    assert!(still_checked(&path, &checked));
    // A file put in its place after the check is not handed on.
    drop(file);
    let _ = std::fs::remove_file(&path);
    std::fs::write(&path, b"swapped in").unwrap();
    assert!(!still_checked(&path, &checked));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn each_download_gets_a_new_private_folder() {
    let (a, b) = (download_dir().unwrap(), download_dir().unwrap());
    assert_ne!(a, b);
    assert!(a.starts_with(std::env::temp_dir()) && a.is_dir() && !std::fs::symlink_metadata(&a).unwrap().file_type().is_symlink());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&a).unwrap().permissions().mode() & 0o777, 0o700);
    }
    // A link at the checked file's name is not opened.
    #[cfg(unix)]
    {
        let target = b.join("elsewhere");
        std::fs::write(&target, b"x").unwrap();
        let link = a.join("gears-1.2.0.zip");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let sum: String = <sha2::Sha256 as sha2::Digest>::digest(b"x").iter().map(|b| std::format!("{b:02x}")).collect();
        assert!(matches!(open_checked(&link, &sum), Err(Refusal::Download(_))));
    }
    let _ = std::fs::remove_dir_all(&a);
    let _ = std::fs::remove_dir_all(&b);
}

fn zip_with(path: &Path, entries: &[(&str, &[u8])], link: Option<&str>) {
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    let options = zip::write::SimpleFileOptions::default();
    for (name, bytes) in entries {
        zip.start_file(*name, options).unwrap();
        zip.write_all(bytes).unwrap();
    }
    if let Some(name) = link {
        zip.add_symlink(name, "/etc/passwd", options).unwrap();
    }
    zip.finish().unwrap();
}

#[test]
fn a_parts_library_unpacks_only_parts() {
    let dir = std::env::temp_dir().join(std::format!("sc-workshop-lib-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let root = dir.join("libraries");
    let good = dir.join("good.zip");
    zip_with(&good, &[("Fasteners/README.md", b"hi"), ("Fasteners/M3/bolt.sc", b"{}"), ("Fasteners/M3/nut.step", b"ISO")], None);
    let shown = library::install(&good, &root, "tangency-labs.fasteners", "1.2.0").unwrap();
    assert_eq!(shown, root.join("tangency-labs.fasteners/1.2.0/Fasteners"));
    assert!(shown.join("M3/bolt.sc").is_file());
    // Installed once: the same version is reused.
    assert_eq!(library::install(&good, &root, "tangency-labs.fasteners", "1.2.0").unwrap(), shown);
    for (name, entries, link) in [
        ("up.zip", vec![("../escape.sc", &b"x"[..])], None),
        ("exe.zip", vec![("Lib/part.sc", &b"x"[..]), ("Lib/setup.exe", &b"MZ"[..])], None),
        ("script.zip", vec![("Lib/run.SH", &b"x"[..])], None),
        ("link.zip", vec![("Lib/part.sc", &b"x"[..])], Some("Lib/passwd.sc")),
    ] {
        let zip = dir.join(name);
        zip_with(&zip, &entries, link);
        let result = library::install(&zip, &root, "maker.bad", "1.0.0");
        assert!(matches!(result, Err(Refusal::LibraryContents(_))), "{name}: {result:?}");
        assert!(!root.join("maker.bad/1.0.0").exists(), "{name}: nothing is left");
    }
    assert!(!dir.join("escape.sc").exists());
    assert!(matches!(library::install(&good, &root, "../x", "1.0.0"), Err(Refusal::BadItem(_))));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_link_from_the_browser_build_or_while_busy_opens_nothing() {
    let mut h = crate::scenario::Harness::new();
    let ctx = egui::Context::default();
    // Not a link: refused with words, nothing started.
    open_link(&mut h.app, &ctx, "solidcraft://workshop/model/a/b?x=1");
    assert!(h.app.workshop.job.is_none() && h.app.workshop.ask.is_none());
    // Without the host's services (the browser, tests) a good link starts nothing either.
    open_link(&mut h.app, &ctx, "solidcraft://workshop/model/a/b");
    assert!(h.app.workshop.job.is_none() && h.app.workshop.ask.is_none());
}

/// The question as it draws: what it shows, and pictures for review with
/// `SOLIDCRAFT_WORKSHOP_EVIDENCE=<folder>`.
#[test]
fn the_question_draws_its_rows_and_buttons() {
    let index = signed(&index_json(&test_key().0));
    for (link, name) in [
        ("solidcraft://workshop/plugin/maker/gears", "plugin"),
        ("solidcraft://workshop/model/bherbruck/wall-mount-shelf-bracket", "model"),
        ("solidcraft://workshop/part/tangency-labs/fasteners", "part"),
    ] {
        let item = run(index.clone(), link, None).unwrap();
        for size in [egui::vec2(1280.0, 720.0), egui::vec2(640.0, 480.0)] {
            let mut h = crate::scenario::Harness::new();
            h.app.workshop.ask = Some(item.clone());
            let ctx = crate::fonts::test_context();
            let mut textures = std::collections::HashMap::new();
            for frame in 0..6 {
                let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
                let mut output =
                    ctx.run_ui(egui::RawInput { screen_rect: Some(bounds), time: Some(frame as f64 / 60.0), ..Default::default() }, |ui| {
                        crate::ui_kit::scope(ui, |ui| show(&mut h.app, ui.ctx()));
                    });
                crate::ui_kit::test_textures_update(&mut textures, &output.textures_delta);
                output.textures_delta.clear();
                if frame == 5 {
                    let text = crate::test_support::text_shapes(&output).map(|shape| shape.galley.text().to_owned()).collect::<Vec<_>>().join("\n");
                    for expected in ["Author", "Tier", "Version", "Size", "Cancel"] {
                        assert!(text.contains(expected), "{name}: {expected} missing in {text}");
                    }
                    if name == "plugin" {
                        assert!(text.contains("commands:sketch") && text.contains("Install"), "{text}");
                    }
                    if let Ok(directory) = std::env::var("SOLIDCRAFT_WORKSHOP_EVIDENCE") {
                        let image = crate::ui_kit::test_raster(&ctx, output.shapes, output.pixels_per_point, bounds, &textures);
                        std::fs::create_dir_all(&directory).unwrap();
                        image.save(Path::new(&directory).join(std::format!("workshop-{name}-{}x{}.png", size.x, size.y))).unwrap();
                    }
                }
            }
            assert!(h.app.workshop.ask.is_some(), "the question stays until it is answered");
        }
    }
}

/// A key pressed just as the question appears (meant for the window) doesn't answer it; the
/// same key a moment later does.
#[test]
fn the_question_ignores_keys_for_its_first_second() {
    let item = run(signed(&index_json(&test_key().0)), "solidcraft://workshop/plugin/maker/gears", None).unwrap();
    let mut h = crate::scenario::Harness::new();
    h.app.workshop.ask = Some(item);
    let ctx = crate::fonts::test_context();
    crate::ui_kit::set_dialog_mnemonics(true);
    let enter = || egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
    let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 720.0));
    let mut frame = |time: f64, events: Vec<egui::Event>| {
        let _ = ctx.run_ui(egui::RawInput { screen_rect: Some(bounds), time: Some(time), events, ..Default::default() }, |ui| {
            crate::ui_kit::scope(ui, |ui| show(&mut h.app, ui.ctx()));
        });
        h.app.workshop.ask.is_some()
    };
    assert!(frame(0.0, Vec::new()));
    assert!(frame(0.1, vec![enter()]), "Enter as the question appears answered it");
    // "Install"'s first letter, with first-letter buttons on.
    assert!(frame(0.5, vec![egui::Event::Text("i".into())]), "a letter as the question appears answered it");
    assert!(frame(0.9, vec![enter(), egui::Event::Text("i".into())]));
    assert!(!frame(1.1, vec![enter()]), "Enter after the delay answers");
    crate::ui_kit::set_dialog_mnemonics(false);
    assert!(h.app.workshop.asked_at.is_none());
}

/// The test index, good until long after the test runs (the window checks freshness against the
/// clock when an item is chosen).
fn lasting_index() -> Value {
    let mut value = index_json(&test_key().0);
    value["expires_at"] = json!("2099-01-01T00:00:00.000Z");
    value
}

fn read_with(answer: Signed, seen: Option<&Path>, app: &semver::Version) -> Result<(index::Index, Vec<solidcraft_engine::workshop::Item>), Refusal> {
    let keys = keys();
    let keys: Vec<(&str, &str)> = keys.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    browser::read(&keys, &|_| Ok(answer.clone()), seen, NOW, app)
}

fn nightly() -> semver::Version {
    semver::Version::parse("0.48.0-nightly.20261126.1").unwrap()
}

/// The window lists what a link could open, at the version a link without one gets: the newest
/// that runs here and isn't blocked. A file from anywhere but the Workshop (or a library's GitHub
/// release) leaves the item out, as the link would refuse it.
#[test]
fn the_window_lists_what_a_link_could_open() {
    use solidcraft_engine::workshop::{Kind as K, Tier as T};
    let (index, items) = read_with(signed(&lasting_index()), None, &nightly()).unwrap();
    assert_eq!(index.sequence, 42);
    let ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["gears", "k3x9", "tangency-labs.fasteners"]);
    let gears = &items[0];
    assert_eq!((gears.kind, gears.tier, gears.version.as_deref()), (K::Plugin, T::Community, Some("1.2.0")));
    assert_eq!(gears.permissions, ["commands:sketch", "files:read"]);
    assert_eq!(gears.warning.as_deref(), Some("Slow on big sketches"));
    assert_eq!((gears.size, gears.author.as_str()), (48211, "A Maker (@maker)"));
    assert_eq!(gears.page_url, "https://workshop.solidcraft.io/plugin/maker/gears");
    // 1.2.0 of the bracket is blocked: the list offers 1.1.0, as the link does.
    assert_eq!((items[1].kind, items[1].version.as_deref(), items[1].tier), (K::Model, Some("1.1.0"), T::Unverified));
    assert_eq!(items[1].page_url, "https://workshop.solidcraft.io/model/bherbruck/wall-mount-shelf-bracket");
    assert_eq!(
        (items[2].kind, items[2].tier, items[2].page_url.as_str()),
        (K::Part, T::Official, "https://workshop.solidcraft.io/part/tangency-labs/fasteners")
    );
    assert_eq!(browser::meta_line(gears), "Plugin · Community · A Maker (@maker) · 47 KB");

    // On an older SolidCraft every version of the plugin needs a newer one: listed, with no
    // version and the version it needs.
    let (_, old) = read_with(signed(&lasting_index()), None, &semver::Version::new(0, 30, 0)).unwrap();
    let gears = old.iter().find(|i| i.id == "gears").unwrap();
    assert_eq!((gears.version.as_deref(), gears.needs_app.as_deref(), gears.size), (None, Some("0.40.0"), 0));
}

/// A huge index is read to its first [`browser::MAX_ITEMS`] items; unknown kinds are skipped.
#[test]
fn a_huge_or_odd_index_is_read_safely() {
    let mut value = lasting_index();
    let many: Vec<Value> = (0..browser::MAX_ITEMS + 50)
        .map(|n| {
            json!({"id": std::format!("m{n}"), "kind": if n == 3 { "theme" } else { "model" }, "owner": "o", "slug": std::format!("m{n}"), "name": "M",
                   "versions": [{"version": "1.0.0", "files": [file("design", "https://workshop.solidcraft.io/api/files/1/download")]}]})
        })
        .collect();
    value["items"] = Value::Array(many);
    let (_, items) = read_with(signed(&value), None, &nightly()).unwrap();
    assert_eq!(items.len(), browser::MAX_ITEMS - 1, "one unknown kind skipped, the rest cut at the limit");
    assert!(items.iter().all(|i| i.id != "m3"));
    // Oversized, empty or zero-byte files, and bad checksums, are left out like any refused item.
    let mut odd = lasting_index();
    odd["items"][0]["versions"] = json!([{"version": "1.0.0", "files": [{"name": "f", "role": "package", "size": 0, "sha256": SHA, "url": "https://workshop.solidcraft.io/x"}]}]);
    odd["items"][1]["versions"] = json!([{"version": "1.0.0", "files": [{"name": "f", "role": "design", "size": u64::MAX, "sha256": "nothex", "url": "https://workshop.solidcraft.io/x"}]}]);
    odd["items"][2]["versions"] = json!([]);
    let (_, items) = read_with(signed(&odd), None, &nightly()).unwrap();
    assert!(items.is_empty(), "{items:?}");
}

/// A read that fails keeps the last good list and says why; a good one replaces it. An older
/// sequence than one seen is a failed read too.
#[test]
fn a_bad_or_replayed_index_keeps_the_last_good_list() {
    let dir = std::env::temp_dir().join(std::format!("sc-workshop-browse-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let seen = dir.join("index-sequence");
    let mut shared = browser::Shared::default();
    browser::settle(&mut shared, read_with(signed(&lasting_index()), Some(&seen), &nightly()));
    assert_eq!(shared.catalog.state, solidcraft_engine::workshop::State::Ready);
    assert_eq!((shared.catalog.items.len(), shared.catalog.sequence), (3, 42));
    let mut tampered = signed(&lasting_index());
    tampered.body.push(b' ');
    browser::settle(&mut shared, read_with(tampered, Some(&seen), &nightly()));
    assert_eq!(shared.catalog.state, solidcraft_engine::workshop::State::Failed);
    assert!(shared.catalog.error.as_deref().is_some_and(|e| !e.is_empty()));
    assert_eq!(shared.catalog.items.len(), 3, "the last good list stays");
    assert!(shared.index.is_some());
    let mut older = lasting_index();
    older["sequence"] = json!(41);
    assert_eq!(read_with(signed(&older), Some(&seen), &nightly()).err(), Some(Refusal::Replayed { seen: 42, got: 41 }));
    assert_eq!(read_with(signed(&lasting_index()), None, &nightly()).map(|(_, i)| i.len()).ok(), Some(3));
    let offline = {
        let keys = keys();
        let keys: Vec<(&str, &str)> = keys.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        browser::read(&keys, &|_| Err("offline".into()), None, NOW, &nightly())
    };
    assert_eq!(offline.err(), Some(Refusal::Unreachable("offline".into())));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Choosing an item asks the link's question, about the version the list showed; nothing starts
/// while another item is asked about, and an item the index doesn't list (or an expired index)
/// is refused with words. Without the host's services (tests, the browser build) the Plugins
/// menu doesn't offer the window and the session has no Workshop.
#[test]
fn choosing_an_item_asks_as_a_link_does() {
    use solidcraft_engine::workshop::Kind as K;
    let mut h = crate::scenario::Harness::new();
    assert!(!browser::offered(&h.app));
    assert!(h.app.session.workshop.is_none());
    assert!(!crate::plugin_menu::rows(&h.app).iter().any(|r| matches!(r, crate::app_bar::FileRow::Item(i) if i.id == "workshop")));
    browser::open(&mut h.app);
    assert!(!h.app.workshop.browser.open, "no window without the Workshop");
    assert!(browser::control(&mut h.app, &json!({"open": true})).is_err());

    browser::settle(&mut browser::lock(&h.app.workshop.browser.shared), read_with(signed(&lasting_index()), None, &nightly()));
    browser::choose(&mut h.app, K::Plugin, "gears");
    let asked = h.app.workshop.ask.clone().expect("the question");
    assert_eq!((asked.id.as_str(), asked.version.to_string()), ("gears", "1.2.0".to_string()));
    assert_eq!(asked.permissions, ["commands:sketch", "files:read"]);
    // Busy: the question stays as it was.
    browser::choose(&mut h.app, K::Model, "k3x9");
    assert_eq!(h.app.workshop.ask.as_ref().map(|a| a.id.as_str()), Some("gears"));
    h.app.workshop.ask = None;
    browser::choose(&mut h.app, K::Model, "k3x9");
    assert_eq!(h.app.workshop.ask.as_ref().map(|a| a.version.to_string()), Some("1.1.0".to_string()));
    h.app.workshop.ask = None;
    // Not listed under that kind, or not at all.
    browser::choose(&mut h.app, K::Model, "gears");
    browser::choose(&mut h.app, K::Plugin, "nothing");
    assert!(h.app.workshop.ask.is_none());
    // An index that has expired since it was read is refused.
    let mut expired = lasting_index();
    expired["expires_at"] = json!("2026-11-20T00:00:00Z");
    let (index, items) = read_with(signed(&expired), None, &nightly()).unwrap();
    browser::settle(&mut browser::lock(&h.app.workshop.browser.shared), Ok((index, items)));
    browser::choose(&mut h.app, K::Plugin, "gears");
    assert!(h.app.workshop.ask.is_none());
}

/// The session's `workshop.list` reads the window's list through the host.
#[test]
fn the_session_lists_what_the_window_shows() {
    let mut h = crate::scenario::Harness::new();
    browser::settle(&mut browser::lock(&h.app.workshop.browser.shared), read_with(signed(&lasting_index()), None, &nightly()));
    h.app.services.workshop_get = Some(Arc::new(|_: &str| Err("offline".to_string())));
    h.app.services.http_download =
        Some(Arc::new(|_: &str, _: &Path, _: &dyn Fn(u64, Option<u64>) -> bool| Err(crate::update::FetchError::Failed("offline".into()))));
    browser::install(&mut h.app);
    assert!(browser::offered(&h.app) == cfg!(not(target_arch = "wasm32")));
    let listed = h.app.session.execute("workshop.list", &json!({"kind": "plugin"})).unwrap();
    assert_eq!(listed["state"], "ready");
    assert_eq!(listed["items"].as_array().map(Vec::len), Some(1));
    assert_eq!(listed["items"][0]["id"], "gears");
    assert!(crate::plugin_menu::rows(&h.app).iter().any(|r| matches!(r, crate::app_bar::FileRow::Item(i) if i.id == "workshop")));

    // `ui.workshop` drives the window as clicks would; bad values are refused.
    let shown = browser::control(&mut h.app, &json!({"open": true, "kind": "plugin"})).unwrap();
    assert_eq!((shown["open"].as_bool(), shown["state"].as_str()), (Some(true), Some("ready")));
    assert_eq!(shown["shown"], json!(["gears"]));
    let searched = browser::control(&mut h.app, &json!({"kind": "all", "query": "shelf"})).unwrap();
    assert_eq!(searched["shown"], json!(["k3x9"]));
    for bad in [
        json!({"kind": "theme"}),
        json!({"query": 3}),
        json!({"query": "x".repeat(201)}),
        json!({"open": "yes"}),
        json!({"choose": "nothing"}),
        json!({"choose": 1}),
    ] {
        assert!(browser::control(&mut h.app, &bad).is_err(), "{bad}");
    }
    let asked = browser::control(&mut h.app, &json!({"choose": "gears"})).unwrap();
    assert_eq!(asked["asking"], "gears");
    assert!(browser::control(&mut h.app, &json!({"open": false})).is_ok_and(|v| v["open"] == false));
}

/// The window as it draws, in English and German: names, versions, badges and buttons, and the
/// search narrowing the list. Pictures for review with `SOLIDCRAFT_WORKSHOP_EVIDENCE=<folder>`.
#[test]
fn the_window_draws_the_list_and_searches_it() {
    for (language, install, tier) in [("en", "Install…", "Community"), ("de", "Installieren…", "Community")] {
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.language = language.into();
        browser::settle(&mut browser::lock(&h.app.workshop.browser.shared), read_with(signed(&lasting_index()), None, &nightly()));
        h.app.workshop.browser.open = true;
        let ctx = crate::fonts::test_context();
        let size = egui::vec2(1280.0, 800.0);
        let mut textures = std::collections::HashMap::new();
        let mut draw = |h: &mut crate::scenario::Harness, name: &str| {
            let mut text = String::new();
            for frame in 0..6 {
                let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
                let mut output =
                    ctx.run_ui(egui::RawInput { screen_rect: Some(bounds), time: Some(frame as f64 / 60.0), ..Default::default() }, |ui| {
                        crate::ui_kit::scope(ui, |ui| browser::show(&mut h.app, ui.ctx()));
                    });
                crate::ui_kit::test_textures_update(&mut textures, &output.textures_delta);
                output.textures_delta.clear();
                if frame == 5 {
                    text = crate::test_support::text_shapes(&output).map(|shape| shape.galley.text().to_owned()).collect::<Vec<_>>().join("\n");
                    if let Ok(directory) = std::env::var("SOLIDCRAFT_WORKSHOP_EVIDENCE") {
                        let image = crate::ui_kit::test_raster(&ctx, output.shapes, output.pixels_per_point, bounds, &textures);
                        std::fs::create_dir_all(&directory).unwrap();
                        image.save(Path::new(&directory).join(std::format!("workshop-browser-{name}-{language}-1280x800.png"))).unwrap();
                    }
                }
            }
            text
        };
        let all = draw(&mut h, "all");
        for expected in ["Gears 1.2.0", "Wall mount shelf bracket 1.1.0", install, tier] {
            assert!(all.contains(expected), "{language}: {expected} missing in {all}");
        }
        assert!(!all.contains("Elsewhere"), "{all}");
        h.app.workshop.browser.query = "shelf".into();
        let searched = draw(&mut h, "search");
        assert!(searched.contains("Wall mount shelf bracket") && !searched.contains("Gears 1.2.0"), "{searched}");
        h.app.workshop.browser.query.clear();
        h.app.workshop.browser.kind = 3;
        let parts = draw(&mut h, "parts");
        assert!(parts.contains("Fasteners") && !parts.contains("Gears 1.2.0"), "{parts}");
        assert!(h.app.workshop.browser.open);
    }
}
