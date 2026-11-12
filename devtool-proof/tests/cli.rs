use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use tempfile::TempDir;

fn cli(root: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_devtool-proof"));
    command.args(args).current_dir(root);
    command
}

fn run(root: &Path, args: &[&str], password: Option<&str>) -> Output {
    let mut command = cli(root, args);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    if password.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command.spawn().unwrap();
    if let Some(password) = password {
        writeln!(child.stdin.take().unwrap(), "{password}").unwrap();
    }
    child.wait_with_output().unwrap()
}

fn ok(root: &Path, args: &[&str], password: Option<&str>) -> Value {
    let output = run(root, args, password);
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn fixture(root: &Path) {
    fs::create_dir_all(root.join("tree/deep/empty-dir")).unwrap();
    fs::write(root.join("tree/a.txt"), "abc").unwrap();
    fs::write(root.join("tree/deep/b.bin"), [0, 1, 2, 3, 255]).unwrap();
    fs::write(root.join("tree/empty"), []).unwrap();
    fs::write(root.join("tree/checksums.sha"), []).unwrap();
}

#[test]
fn known_sha_vectors_and_stdin() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::write(root.join("abc"), "abc").unwrap();
    assert_eq!(
        ok(root, &["hash", "abc"], None)["hash"],
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        ok(root, &["hash", "abc", "--algorithm", "sha512"], None)["hash"],
        "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
    );
    let output = cli(root, &["hash", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let Output { stdout, status, .. } = {
        let mut child = output;
        child.stdin.take().unwrap().write_all(b"abc").unwrap();
        child.wait_with_output().unwrap()
    };
    assert!(status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&stdout).unwrap()["bytes"],
        3
    );
}

#[test]
fn sha_manifest_self_exclusion_sorting_gnu_and_verification() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fixture(root);
    let report = ok(root, &["manifest", "create", "tree"], None);
    assert_eq!(report["entries"], 3);
    let original = fs::read(root.join("tree/checksums.sha")).unwrap();
    ok(root, &["manifest", "create", "tree"], None);
    assert_eq!(fs::read(root.join("tree/checksums.sha")).unwrap(), original);
    assert_eq!(
        ok(root, &["manifest", "verify", "tree/checksums.sha"], None)["valid"],
        true
    );
    let external = Command::new("sha256sum")
        .args(["-c", "checksums.sha"])
        .current_dir(root.join("tree"))
        .output()
        .unwrap();
    assert!(
        external.status.success(),
        "{}",
        String::from_utf8_lossy(&external.stderr)
    );
    fs::write(root.join("tree/a.txt"), "changed").unwrap();
    fs::remove_file(root.join("tree/deep/b.bin")).unwrap();
    let output = run(root, &["manifest", "verify", "tree/checksums.sha"], None);
    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["mismatches"], serde_json::json!(["a.txt"]));
    assert_eq!(report["missing"], serde_json::json!(["deep/b.bin"]));
}

#[test]
fn sha_escaped_filenames_and_sha512() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::create_dir(root.join("tree")).unwrap();
    for name in ["space name", "slash\\name", "line\nname", "carriage\r"] {
        fs::write(root.join("tree").join(name), "content").unwrap();
    }
    ok(
        root,
        &["manifest", "create", "tree", "--algorithm", "sha512"],
        None,
    );
    assert_eq!(
        ok(root, &["manifest", "verify", "tree/checksums.sha"], None)["checked"],
        4
    );
    let output = Command::new("sha512sum")
        .args(["-c", "checksums.sha"])
        .current_dir(root.join("tree"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn json_manifest_compare_tracks_added_removed_and_changed() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fixture(root);
    ok(
        root,
        &[
            "manifest", "create", "tree", "--format", "json", "-o", "old.json",
        ],
        None,
    );
    fs::write(root.join("tree/a.txt"), "new").unwrap();
    fs::remove_file(root.join("tree/empty")).unwrap();
    fs::write(root.join("tree/new"), "new").unwrap();
    ok(
        root,
        &[
            "manifest", "create", "tree", "--format", "json", "-o", "new.json",
        ],
        None,
    );
    let report = ok(root, &["manifest", "compare", "old.json", "new.json"], None);
    assert_eq!(report["added"], serde_json::json!(["new"]));
    assert_eq!(report["removed"], serde_json::json!(["empty"]));
    assert_eq!(report["changed"], serde_json::json!(["a.txt"]));
    ok(
        root,
        &["manifest", "verify", "new.json", "--root", "tree"],
        None,
    );
}

#[test]
fn signatures_verify_and_reject_modified_data_wrong_keys_and_signature() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::write(root.join("data"), "original").unwrap();
    ok(
        root,
        &[
            "key",
            "generate",
            "--private",
            "private.json",
            "--public",
            "public.json",
        ],
        None,
    );
    ok(root, &["sign", "data", "--key", "private.json"], None);
    ok(
        root,
        &["verify-signature", "data", "--key", "public.json"],
        None,
    );
    ok(
        root,
        &[
            "key",
            "generate",
            "--private",
            "other-private",
            "--public",
            "other-public",
        ],
        None,
    );
    assert_eq!(
        run(
            root,
            &["verify-signature", "data", "--key", "other-public"],
            None
        )
        .status
        .code(),
        Some(1)
    );
    fs::write(root.join("data"), "tampered").unwrap();
    assert_eq!(
        run(
            root,
            &["verify-signature", "data", "--key", "public.json"],
            None
        )
        .status
        .code(),
        Some(1)
    );
    fs::write(root.join("data"), "original").unwrap();
    let mut signature: Value =
        serde_json::from_slice(&fs::read(root.join("data.sig")).unwrap()).unwrap();
    signature["signature"] = "00".repeat(64).into();
    fs::write(
        root.join("data.sig"),
        serde_json::to_vec(&signature).unwrap(),
    )
    .unwrap();
    assert_eq!(
        run(
            root,
            &["verify-signature", "data", "--key", "public.json"],
            None
        )
        .status
        .code(),
        Some(1)
    );
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        fs::metadata(root.join("private.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn provenance_snapshot_metadata_signature_and_tamper_detection() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fixture(root);
    ok(
        root,
        &[
            "provenance",
            "create",
            "tree",
            "-o",
            "provenance.json",
            "--run",
            "run-nr.001",
            "--command",
            "devtool --canonical-tree",
            "--tool",
            "devtool=0.4.0",
            "--input",
            "a.txt",
            "--artifact",
            "deep/b.bin",
        ],
        None,
    );
    let document = ok(root, &["provenance", "show", "provenance.json"], None);
    let duplicate = run(
        root,
        &[
            "provenance",
            "create",
            "tree",
            "-o",
            "duplicate.json",
            "--input",
            "a.txt",
            "--input",
            "a.txt",
        ],
        None,
    );
    assert_eq!(duplicate.status.code(), Some(2));
    assert!(!root.join("duplicate.json").exists());
    assert_eq!(document["run"], "run-nr.001");
    assert_eq!(document["tools"]["devtool-proof"], "0.2.0");
    assert_eq!(document["declared_command"], "devtool --canonical-tree");
    ok(
        root,
        &["provenance", "verify", "provenance.json", "--root", "tree"],
        None,
    );
    fs::write(root.join("tree/a.txt"), "tampered").unwrap();
    assert_eq!(
        run(
            root,
            &["provenance", "verify", "provenance.json", "--root", "tree"],
            None
        )
        .status
        .code(),
        Some(1)
    );
}

fn archive_roundtrip(format: &str, names: bool) {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fixture(root);
    let name = format!("archive.{format}");
    let mut args = vec![
        "archive",
        "encrypt",
        "tree",
        "--format",
        format,
        "--password-stdin",
        "-o",
        &name,
    ];
    if names {
        args.push("--encrypt-names");
    }
    let report = ok(root, &args, Some(" a password ü "));
    assert_eq!(report["cipher"], "aes256");
    ok(
        root,
        &["archive", "test", &name, "--password-stdin"],
        Some(" a password ü "),
    );
    assert!(
        !run(
            root,
            &["archive", "test", &name, "--password-stdin"],
            Some("wrong")
        )
        .status
        .success()
    );
    ok(
        root,
        &[
            "archive",
            "decrypt",
            &name,
            "--password-stdin",
            "-o",
            "unpacked",
        ],
        Some(" a password ü "),
    );
    for name in ["a.txt", "deep/b.bin", "empty", "checksums.sha"] {
        assert_eq!(
            fs::read(root.join("tree").join(name)).unwrap(),
            fs::read(root.join("unpacked/tree").join(name)).unwrap()
        );
    }
    assert!(root.join("unpacked/tree/deep/empty-dir").is_dir());
    if format == "7z" {
        let header = sevenz_rust2::ArchiveReader::new(
            fs::File::open(root.join(&name)).unwrap(),
            sevenz_rust2::Password::empty(),
        );
        assert_eq!(header.is_err(), names);
    }
    if format == "zip" {
        let mut archive =
            zip::ZipArchive::new(fs::File::open(root.join("archive.zip")).unwrap()).unwrap();
        let file = archive.by_index_raw(1).unwrap();
        assert!(file.encrypted());
        let extra = file.extra_data().unwrap();
        let mut offset = 0;
        let mut aes256 = false;
        while offset + 4 <= extra.len() {
            let tag = u16::from_le_bytes([extra[offset], extra[offset + 1]]);
            let length = u16::from_le_bytes([extra[offset + 2], extra[offset + 3]]) as usize;
            if tag == 0x9901 {
                aes256 = extra[offset + 8] == 3;
            }
            offset += 4 + length;
        }
        assert!(aes256);
        let magic = fs::read(root.join("archive.zip")).unwrap();
        assert!(magic.starts_with(b"PK"));
    }
}

#[test]
fn zip_aes256_roundtrip_and_wrong_password() {
    archive_roundtrip("zip", false);
}

#[test]
fn sevenz_aes256_headers_roundtrip_and_wrong_password() {
    archive_roundtrip("7z", true);
}

#[test]
fn sevenz_visible_headers_roundtrip_and_wrong_password() {
    archive_roundtrip("7z", false);
}

#[test]
fn password_file_and_archive_tampering() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fixture(root);
    fs::write(root.join("password"), "file-password\n").unwrap();
    ok(
        root,
        &[
            "archive",
            "encrypt",
            "tree",
            "--format",
            "zip",
            "--password-file",
            "password",
            "-o",
            "data.zip",
        ],
        None,
    );
    ok(
        root,
        &["archive", "test", "data.zip", "--password-file", "password"],
        None,
    );
    let mut bytes = fs::read(root.join("data.zip")).unwrap();
    let mut archive = zip::ZipArchive::new(fs::File::open(root.join("data.zip")).unwrap()).unwrap();
    let entry = archive.by_index_raw(1).unwrap();
    let offset = entry.data_start().unwrap() as usize;
    bytes[offset + 20] ^= 0x80;
    fs::write(root.join("data.zip"), bytes).unwrap();
    assert!(
        !run(
            root,
            &["archive", "test", "data.zip", "--password-file", "password"],
            None
        )
        .status
        .success()
    );
}

#[test]
fn schemas_and_invalid_archive_options() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fixture(root);
    for kind in ["manifest", "key", "signature", "provenance"] {
        assert!(ok(root, &["schema", "--kind", kind], None)["$schema"].is_string());
    }
    assert!(
        !run(
            root,
            &[
                "archive",
                "encrypt",
                "tree",
                "--format",
                "zip",
                "--encrypt-names",
                "--password-stdin",
                "-o",
                "bad.zip"
            ],
            Some("pw")
        )
        .status
        .success()
    );
    assert!(!root.join("bad.zip").exists());
    assert_eq!(run(root, &["hash", "missing"], None).status.code(), Some(2));
}

#[test]
fn ed25519_signature_is_verified_independently_by_openssl() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::write(root.join("data"), "independent verification").unwrap();
    ok(
        root,
        &[
            "key",
            "generate",
            "--private",
            "private.json",
            "--public",
            "public.json",
        ],
        None,
    );
    ok(root, &["sign", "data", "--key", "private.json"], None);
    let envelope: Value =
        serde_json::from_slice(&fs::read(root.join("data.sig")).unwrap()).unwrap();
    let mut der = hex::decode("302a300506032b6570032100").unwrap();
    der.extend(hex::decode(envelope["public_key"].as_str().unwrap()).unwrap());
    fs::write(root.join("public.der"), der).unwrap();
    let mut message = b"devtool-proof.signature/1\0sha256\0".to_vec();
    message.extend(hex::decode(envelope["hash"].as_str().unwrap()).unwrap());
    fs::write(root.join("message"), message).unwrap();
    fs::write(
        root.join("signature"),
        hex::decode(envelope["signature"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    let output = Command::new("openssl")
        .args([
            "pkeyutl",
            "-verify",
            "-pubin",
            "-inkey",
            "public.der",
            "-keyform",
            "DER",
            "-rawin",
            "-in",
            "message",
            "-sigfile",
            "signature",
        ])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn explicit_derived_outputs_keep_live_state_out_of_evidence() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fixture(root);
    fs::write(root.join("tree/state.txt"), "sha_checksum: false\n").unwrap();
    ok(
        root,
        &["manifest", "create", "tree", "--derived", "state.txt"],
        None,
    );
    ok(
        root,
        &[
            "provenance",
            "create",
            "tree",
            "--derived",
            "state.txt",
            "-o",
            "provenance.json",
        ],
        None,
    );
    fs::write(root.join("tree/state.txt"), "sha_checksum: true\n").unwrap();
    ok(root, &["manifest", "verify", "tree/checksums.sha"], None);
    ok(
        root,
        &["provenance", "verify", "provenance.json", "--root", "tree"],
        None,
    );
    ok(
        root,
        &[
            "manifest", "create", "tree", "--format", "json", "-o", "all.json",
        ],
        None,
    );
    fs::write(root.join("tree/state.txt"), "sha_checksum: false\n").unwrap();
    assert_eq!(
        run(
            root,
            &["manifest", "verify", "all.json", "--root", "tree"],
            None
        )
        .status
        .code(),
        Some(1)
    );
}
