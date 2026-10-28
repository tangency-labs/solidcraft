use super::install::*;
use super::{Asset, FetchError, InstallKind, Release};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// A key pair made with `minisign -G -W` for these tests, and its signature of [`SIGNED_SUMS`]
/// with the trusted comment "SolidCraft v9.9.9".
const TEST_KEY: &str = "RWSnDqhIJ/zGKMSbsGUl4/prZLXqi3PwpEeY/D8gvdsEUeZRO9yWCoSM";
const SIGNED_SUMS: &str = "aaaa  solidcraft-linux-x86_64.AppImage\n";
const SIGNATURE: &str = "untrusted comment: signature from minisign secret key\nRUSnDqhIJ/zGKEyvbowsT8vEGvdv2OL9Rwv4bo2m7pJC54t2UQWXjU0+V7ayPB0Okk1teY53qZSTugz2Dnko35A3Xc1jdNFZCwU=\ntrusted comment: SolidCraft v9.9.9\npCRA0RNYxyt4SSqLbfI3p00xsCE4Rt8ixJWAQfoyb226cz/XdS7A2PRdG7hFmRmfg4h6zmtIt3iRLSKjk6RsAQ==\n";

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("solidcraft-update-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn checksum_lists_are_read() {
    let h = "a".repeat(64);
    let sums = format!(
        "{h}  solidcraft-linux-x86_64.AppImage\n{}  *solidcraft-windows-x64.msi\nnot a line\n{}  short.txt\n",
        "B".repeat(64),
        "c".repeat(10)
    );
    assert_eq!(expected_sha256(&sums, "solidcraft-linux-x86_64.AppImage"), Some(h));
    assert_eq!(expected_sha256(&sums, "solidcraft-windows-x64.msi"), Some("b".repeat(64)), "binary-mode marker and upper case");
    assert_eq!(expected_sha256(&sums, "short.txt"), None, "not a SHA-256");
    assert_eq!(expected_sha256(&sums, "solidcraft-linux-x86_64"), None);
}

#[test]
fn signatures_must_match_the_key_the_list_and_the_release() {
    assert_eq!(verify_signature(SIGNED_SUMS.as_bytes(), SIGNATURE, TEST_KEY, "v9.9.9"), Ok(()));
    assert!(verify_signature(SIGNED_SUMS.as_bytes(), SIGNATURE, TEST_KEY, "v9.9.8").is_err(), "an older release's signed list");
    assert!(verify_signature(b"bbbb  solidcraft-linux-x86_64.AppImage\n", SIGNATURE, TEST_KEY, "v9.9.9").is_err(), "a changed list");
    let other = "RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
    assert!(verify_signature(SIGNED_SUMS.as_bytes(), SIGNATURE, other, "v9.9.9").is_err(), "another key");
    assert!(verify_signature(SIGNED_SUMS.as_bytes(), "garbage", TEST_KEY, "v9.9.9").is_err());
}

#[test]
fn methods_per_installation() {
    let exe = Path::new("/x/solidcraft.exe");
    assert_eq!(method(&InstallKind::AppImage("/a/S.AppImage".into()), Some(exe)), Some(Method::AppImage("/a/S.AppImage".into())));
    assert_eq!(method(&InstallKind::WindowsMsi, Some(exe)), Some(Method::Msi));
    assert_eq!(method(&InstallKind::WindowsPortable, Some(exe)), Some(Method::PortableZip(exe.into())));
    assert_eq!(method(&InstallKind::WindowsExe, Some(exe)), Some(Method::Exe(exe.into())));
    assert_eq!(method(&InstallKind::MacApp("/A/S.app".into()), Some(exe)), Some(Method::Dmg));
    assert_eq!(
        method(&InstallKind::LinuxTarball, Some(Path::new("/opt/solidcraft-0.1.0/bin/solidcraft"))),
        Some(Method::Tarball("/opt/solidcraft-0.1.0".into()))
    );
    assert_eq!(tarball_prefix(Path::new("/opt/solidcraft")), Some(PathBuf::from("/opt")), "a program not inside bin/");
    assert_eq!(tarball_prefix(Path::new("/solidcraft")), None, "the filesystem root is not an install folder");
    assert_eq!(method(&InstallKind::LinuxTarball, Some(Path::new("/opt/bin/solidcraft"))), None, "a top-level folder is not replaced");
    assert_eq!(method(&InstallKind::LinuxTarball, None), None, "no program path, so no folder to replace");
    for never in [
        InstallKind::Flatpak("id".into()),
        InstallKind::Snap { store: true },
        InstallKind::Snap { store: false },
        InstallKind::Deb,
        InstallKind::Rpm,
        InstallKind::Other,
        InstallKind::Dev,
    ] {
        assert_eq!(method(&never, Some(exe)), None, "{never:?} is never replaced in place");
    }
}

const BASE: &str = "https://github.com/bherbruck/solidcraft/releases/download/v9.9.9/";

fn release_with(assets: &[(&str, usize)]) -> Release {
    Release {
        tag: "v9.9.9".into(),
        name: "SolidCraft v9.9.9".into(),
        notes: String::new(),
        page: "https://github.com/bherbruck/solidcraft/releases/tag/v9.9.9".into(),
        assets: assets.iter().map(|(n, size)| Asset { name: n.to_string(), url: format!("{BASE}{n}"), size: *size as u64 }).collect(),
    }
}

/// A network that serves `files` by name.
fn net(files: Vec<(String, Vec<u8>)>) -> Net {
    let files = Arc::new(files);
    let f2 = files.clone();
    let find = move |files: &[(String, Vec<u8>)], url: &str| files.iter().find(|(n, _)| url == format!("{BASE}{n}")).map(|(_, b)| b.clone());
    let find2 = find;
    Net {
        get: Arc::new(move |url: &str| find(&files, url).ok_or_else(|| FetchError::Failed(format!("404 {url}")))),
        download: Arc::new(move |url: &str, to: &Path, progress: &dyn Fn(u64, Option<u64>) -> bool| {
            let body = find2(&f2, url).ok_or_else(|| FetchError::Failed(format!("404 {url}")))?;
            let mut file = std::fs::File::create(to).map_err(|e| FetchError::Failed(e.to_string()))?;
            for chunk in body.chunks(4) {
                file.write_all(chunk).map_err(|e| FetchError::Failed(e.to_string()))?;
                if !progress(chunk.len() as u64, Some(body.len() as u64)) {
                    return Err(FetchError::Failed("cancelled".into()));
                }
            }
            Ok(())
        }),
    }
}

#[test]
fn an_appimage_is_replaced_only_when_it_verifies() {
    let dir = scratch("appimage");
    let target = dir.join("SolidCraft.AppImage");
    std::fs::write(&target, b"old version").unwrap();
    let new = b"new version".to_vec();
    let name = "solidcraft-linux-x86_64.AppImage";
    let kind = InstallKind::AppImage(target.clone());
    let m = Method::AppImage(target.clone());
    let r = release_with(&[(name, new.len()), (SUMS, 100)]);

    // A package that doesn't match the list: deleted, the old one untouched.
    let wrong = format!("{}  {name}\n", hex(b"something else"));
    let n = net(vec![(name.into(), new.clone()), (SUMS.into(), wrong.into_bytes())]);
    let err = run(&r, &kind, &m, "x86_64", &n, &dir.join("staging"), &Progress::default()).unwrap_err();
    assert!(err.contains("doesn't match"), "{err}");
    assert_eq!(std::fs::read(&target).unwrap(), b"old version");
    assert!(!dir.join("SolidCraft.AppImage.update").exists(), "the bad download is deleted");

    // Not listed at all.
    let n = net(vec![(name.into(), new.clone()), (SUMS.into(), b"".to_vec())]);
    assert!(run(&r, &kind, &m, "x86_64", &n, &dir, &Progress::default()).unwrap_err().contains("doesn't list"));
    // No checksum list in the release: refused before downloading.
    let bare = release_with(&[(name, new.len())]);
    assert!(run(&bare, &kind, &m, "x86_64", &n, &dir, &Progress::default()).unwrap_err().contains(SUMS));

    // Cancelled: nothing changes.
    let good = format!("{}  {name}\n", hex(&new));
    let n = net(vec![(name.into(), new.clone()), (SUMS.into(), good.clone().into_bytes())]);
    let p = Progress::default();
    p.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(run(&r, &kind, &m, "x86_64", &n, &dir, &p).is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"old version");

    // Verified: renamed over the old one, executable, restart offered.
    let out = run(&r, &kind, &m, "x86_64", &n, &dir, &Progress::default()).unwrap();
    assert_eq!(out, Outcome::Restart(target.clone()));
    assert_eq!(std::fs::read(&target).unwrap(), new);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&target).unwrap().permissions().mode() & 0o777, 0o755);
    }
    assert_eq!(std::fs::read_dir(&dir).unwrap().filter_map(Result::ok).filter(|e| e.file_name().to_string_lossy().ends_with(".update")).count(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_portable_zip_swaps_both_executables() {
    let dir = scratch("portable");
    let exe = dir.join("solidcraft.exe");
    std::fs::write(&exe, b"old app").unwrap();
    std::fs::write(dir.join("solidcraft-cli.exe"), b"old cli").unwrap();
    let mut zip_bytes = Vec::new();
    {
        let mut z = zip::ZipWriter::new(std::io::Cursor::new(&mut zip_bytes));
        let o = zip::write::SimpleFileOptions::default();
        z.add_directory("solidcraft-9.9.9-windows-x64-portable/", o).unwrap();
        for (n, b) in [("solidcraft.exe", &b"new app"[..]), ("solidcraft-cli.exe", b"new cli"), ("README.md", b"readme")] {
            z.start_file(format!("solidcraft-9.9.9-windows-x64-portable/{n}"), o).unwrap();
            z.write_all(b).unwrap();
        }
        z.finish().unwrap();
    }
    let name = "solidcraft-windows-x64-portable.zip";
    let r = release_with(&[(name, zip_bytes.len()), (SUMS, 100)]);
    let n = net(vec![(name.into(), zip_bytes.clone()), (SUMS.into(), format!("{}  {name}\n", hex(&zip_bytes)).into_bytes())]);
    let out =
        run(&r, &InstallKind::WindowsPortable, &Method::PortableZip(exe.clone()), "x86_64", &n, &dir.join("staging"), &Progress::default()).unwrap();
    assert_eq!(out, Outcome::Restart(exe.clone()));
    assert_eq!(std::fs::read(&exe).unwrap(), b"new app");
    assert_eq!(std::fs::read(dir.join("solidcraft-cli.exe")).unwrap(), b"new cli");
    assert!(!dir.join("staging").join(name).exists(), "the zip is removed");
    // A zip without the app is refused and changes nothing.
    std::fs::write(dir.join("empty.zip"), {
        let mut b = Vec::new();
        zip::ZipWriter::new(std::io::Cursor::new(&mut b)).finish().unwrap();
        b
    })
    .unwrap();
    assert!(apply(&Method::PortableZip(exe.clone()), &dir.join("empty.zip")).is_err());
    assert_eq!(std::fs::read(&exe).unwrap(), b"new app");
    // The moved-aside executable of an update goes at the next start.
    std::fs::write(dir.join("solidcraft.exe.old"), b"x").unwrap();
    clean_up_after_update(Some(&exe));
    assert!(!dir.join("solidcraft.exe.old").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn installers_and_disk_images_are_handed_over() {
    let dir = scratch("msi");
    let pkg = dir.join("solidcraft-windows-x64.msi");
    std::fs::write(&pkg, b"msi").unwrap();
    assert_eq!(
        apply(&Method::Msi, &pkg).unwrap(),
        Outcome::Installer("msiexec".into(), vec!["/i".into(), pkg.to_string_lossy().to_string(), "/passive".into()])
    );
    assert_eq!(apply(&Method::Dmg, &pkg).unwrap(), Outcome::Opened(pkg.clone()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn windows_installer_is_followed_by_the_new_version() {
    let args = vec!["/i".to_string(), r"C:\Users\a b\Temp\solidcraft-update\x.msi".to_string(), "/passive".to_string()];
    let (program, out) = relaunch_after(Path::new("msiexec"), &args, Path::new(r"C:\Program Files\SolidCraft\it's.exe"));
    assert_eq!(program, PathBuf::from("powershell"));
    assert_eq!(
        out.last().unwrap(),
        &concat!(
            r#"Start-Process -FilePath 'msiexec' -ArgumentList '/i','"C:\Users\a b\Temp\solidcraft-update\x.msi"','/passive' -Wait; "#,
            r"Start-Process -FilePath 'C:\Program Files\SolidCraft\it''s.exe'"
        )
    );
    assert!(out.iter().any(|a| a == "-Command"));
}

fn gzip_bytes(raw: &[u8]) -> Vec<u8> {
    let mut encoded = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoded.write_all(raw).unwrap();
    encoded.finish().unwrap()
}

/// A ustar archive. `typeflag` is `b'0'` for a file. Used when the `tar` builder would refuse the name.
fn raw_tar(entries: &[(&str, u8, &[u8])]) -> Vec<u8> {
    let mut archive = Vec::new();
    for (name, typeflag, data) in entries {
        assert!(name.len() <= 100, "{name}");
        let mut header = vec![0u8; 512];
        header[..name.len()].copy_from_slice(name.as_bytes());
        let octal = |value: u64, width: usize| {
            let text = format!("{:0width$o}", value, width = width - 1);
            let mut bytes = text.into_bytes();
            bytes.push(0);
            bytes
        };
        header[100..108].copy_from_slice(&octal(0o755, 8));
        header[108..116].copy_from_slice(&octal(0, 8));
        header[116..124].copy_from_slice(&octal(0, 8));
        header[124..136].copy_from_slice(&octal(data.len() as u64, 12));
        header[136..148].copy_from_slice(&octal(0, 12));
        header[148..156].copy_from_slice(b"        ");
        header[156] = *typeflag;
        header[257..262].copy_from_slice(b"ustar");
        header[263] = 0;
        header[264..266].copy_from_slice(b"00");
        let sum: u32 = header.iter().map(|b| u32::from(*b)).sum();
        let checksum = format!("{sum:06o}\0 ");
        header[148..156].copy_from_slice(checksum.as_bytes());
        archive.extend_from_slice(&header);
        archive.extend_from_slice(data);
        archive.extend(std::iter::repeat(0u8).take((512 - (data.len() % 512)) % 512));
    }
    archive.extend(std::iter::repeat(0u8).take(1024));
    archive
}

/// A gzip-compressed tar archive of `files` (`name` is the path inside the archive).
fn gzip_tar(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut raw = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut raw);
        for (name, bytes) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder.append_data(&mut header, *name, *bytes).unwrap();
        }
        builder.finish().unwrap();
    }
    let mut encoded = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoded.write_all(&raw).unwrap();
    encoded.finish().unwrap()
}

#[test]
fn a_tarball_replaces_its_install_folder_only_when_it_verifies() {
    let dir = scratch("tarball");
    let prefix = dir.join("solidcraft-9.9.8");
    let exe = prefix.join("bin").join("solidcraft");
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
    std::fs::write(&exe, b"old version").unwrap();
    std::fs::write(prefix.join("bin").join("solidcraft-cli"), b"old cli").unwrap();

    let name = "solidcraft-9.9.9-linux-x86_64.tar.gz";
    let payload = gzip_tar(&[
        ("solidcraft-9.9.9-linux-x86_64/bin/solidcraft", b"new version"),
        ("solidcraft-9.9.9-linux-x86_64/bin/solidcraft-cli", b"new cli"),
        ("solidcraft-9.9.9-linux-x86_64/share/doc/README.md", b"notes"),
    ]);
    let release = release_with(&[(name, payload.len()), (SUMS, 100)]);
    let method = Method::Tarball(prefix.clone());

    let wrong = format!("{}  {name}\n", hex(b"something else"));
    let bad = net(vec![(name.into(), payload.clone()), (SUMS.into(), wrong.into_bytes())]);
    let err = run(&release, &InstallKind::LinuxTarball, &method, "x86_64", &bad, &dir.join("staging"), &Progress::default()).unwrap_err();
    assert!(err.contains("doesn't match"), "{err}");
    assert_eq!(std::fs::read(&exe).unwrap(), b"old version", "a bad checksum leaves the install");
    assert!(!dir.join("solidcraft-9.9.8.old").exists());

    let sums = format!("{}  {name}\n", hex(&payload));
    let good = net(vec![(name.into(), payload), (SUMS.into(), sums.into_bytes())]);
    let out = run(&release, &InstallKind::LinuxTarball, &method, "x86_64", &good, &dir.join("staging"), &Progress::default()).unwrap();
    assert_eq!(out, Outcome::Restart(exe.clone()));
    assert_eq!(std::fs::read(&exe).unwrap(), b"new version");
    assert_eq!(std::fs::read(prefix.join("bin/solidcraft-cli")).unwrap(), b"new cli");
    assert_eq!(std::fs::read(prefix.join("share/doc/README.md")).unwrap(), b"notes");
    let backup = dir.join("solidcraft-9.9.8.old");
    assert_eq!(std::fs::read(backup.join("bin/solidcraft")).unwrap(), b"old version", "the previous folder is kept until the next start");
    clean_up_after_update(Some(&exe));
    assert!(!backup.exists(), "the next start removes the previous folder");

    // The tar builder refuses to write `..`, so this header is assembled directly.
    let evil = gzip_bytes(&raw_tar(&[("solidcraft-9.9.9-linux-x86_64/../../escaped", b'0', b"bad")]));
    let evil_path = dir.join("evil.tar.gz");
    std::fs::write(&evil_path, &evil).unwrap();
    let err = apply(&Method::Tarball(prefix.clone()), &evil_path).unwrap_err();
    assert!(err.contains("unsafe"), "{err}");
    assert!(!dir.join("escaped").exists());
    assert!(!dir.join("solidcraft-9.9.8.update-new").exists());
    assert_eq!(std::fs::read(&exe).unwrap(), b"new version");

    let empty = gzip_tar(&[("solidcraft-9.9.9-linux-x86_64/README.md", b"only notes")]);
    let empty_path = dir.join("empty.tar.gz");
    std::fs::write(&empty_path, &empty).unwrap();
    assert!(apply(&Method::Tarball(prefix.clone()), &empty_path).unwrap_err().contains("no solidcraft program"));
    assert_eq!(std::fs::read(&exe).unwrap(), b"new version");
    assert!(!dir.join("solidcraft-9.9.8.update-new").exists());

    let mut raw = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut raw);
        let mut header = tar::Header::new_gnu();
        header.set_path("solidcraft-9.9.9-linux-x86_64/bin/solidcraft").unwrap();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        header.set_link_name("elsewhere").unwrap();
        header.set_cksum();
        builder.append(&header, std::io::empty()).unwrap();
        builder.finish().unwrap();
    }
    let mut encoded = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoded.write_all(&raw).unwrap();
    let link_path = dir.join("link.tar.gz");
    std::fs::write(&link_path, encoded.finish().unwrap()).unwrap();
    let err = apply(&Method::Tarball(prefix.clone()), &link_path).unwrap_err();
    assert!(err.contains("refusing"), "{err}");
    assert_eq!(std::fs::read(&exe).unwrap(), b"new version", "a symlink archive does not replace the program");
    assert!(!dir.join("solidcraft-9.9.8.update-new").exists());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_updated_appimage_takes_the_release_file_name() {
    let new = |p: &str| release_named(Path::new(p), "v0.17.0", "x86_64");
    assert_eq!(new("/a/solidcraft-0.16.2-linux-x86_64.AppImage"), Path::new("/a/solidcraft-0.17.0-linux-x86_64.AppImage"));
    assert_eq!(new("/a/solvecraft-0.9.0-linux-x86_64.AppImage"), Path::new("/a/solidcraft-0.17.0-linux-x86_64.AppImage"));
    assert_eq!(new("/a/SolidCraft.AppImage"), Path::new("/a/SolidCraft.AppImage"), "a name the user chose");
    assert_eq!(new("/a/solidcraft-linux-x86_64.AppImage"), Path::new("/a/solidcraft-linux-x86_64.AppImage"), "no version in it");
    assert_eq!(new("/a/solidcraft-0.16.2-linux-x86_64.AppImage.bak"), Path::new("/a/solidcraft-0.16.2-linux-x86_64.AppImage.bak"));

    // End to end: the old versioned file is gone, the new one has the release's name and works.
    let dir = scratch("appimage-name");
    let old = dir.join("solidcraft-9.9.8-linux-x86_64.AppImage");
    std::fs::write(&old, b"old version").unwrap();
    let name = "solidcraft-linux-x86_64.AppImage";
    let content = b"new version".to_vec();
    let r = release_with(&[(name, content.len()), (SUMS, 100)]);
    let sums = format!("{}  {name}\n", hex(&content));
    let n = net(vec![(name.into(), content.clone()), (SUMS.into(), sums.into_bytes())]);
    let kind = InstallKind::AppImage(old.clone());
    let out = run(&r, &kind, &Method::AppImage(old.clone()), "x86_64", &n, &dir, &Progress::default()).unwrap();
    let want = dir.join("solidcraft-9.9.9-linux-x86_64.AppImage");
    assert_eq!(out, Outcome::Restart(want.clone()));
    assert_eq!(std::fs::read(&want).unwrap(), content);
    assert!(!old.exists());
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "nothing else is left behind");
    let _ = std::fs::remove_dir_all(&dir);
}
