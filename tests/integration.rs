use assert_cmd::Command;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::thread;
use tempfile::TempDir;

struct Fixture {
    dir: TempDir,
    pieces_dir: PathBuf,
    manifest_path: PathBuf,
    out_dir: PathBuf,
    piece_paths: Vec<PathBuf>,
    piece_data: Vec<Vec<u8>>,
}

fn sha_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

fn build_fixture(sizes: &[usize], file_names: &[String], url_names: &[String]) -> Fixture {
    assert_eq!(sizes.len(), file_names.len());
    assert_eq!(sizes.len(), url_names.len());

    let dir = tempfile::tempdir().unwrap();
    let pieces_dir = dir.path().join("pieces");
    let out_dir = dir.path().join("out");
    fs::create_dir_all(&pieces_dir).unwrap();
    fs::create_dir_all(&out_dir).unwrap();

    let mut offset = 0u64;
    let mut piece_data = Vec::new();
    let mut piece_paths = Vec::new();
    let mut pieces = Vec::new();

    for (index, size) in sizes.iter().enumerate() {
        let data = vec![(index as u8) + 1; *size];
        let hash = sha_hex(&data);
        let path = pieces_dir.join(&file_names[index]);
        fs::write(&path, &data).unwrap();
        piece_paths.push(path);
        piece_data.push(data);
        pieces.push(json!({
            "url": format!("https://example.invalid/{}", url_names[index]),
            "fileOffset": offset,
            "fileSize": *size as u64,
            "hashValue": hash,
        }));
        offset += *size as u64;
    }

    let all: Vec<u8> = piece_data.iter().flat_map(|data| data.clone()).collect();
    let manifest = json!({
        "originalFileSize": offset,
        "packageDigest": sha_hex(&all),
        "numberOfSplitFiles": sizes.len(),
        "pieces": pieces,
    });
    let manifest_path = pieces_dir.join("game.json");
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    Fixture {
        dir,
        pieces_dir,
        manifest_path,
        out_dir,
        piece_paths,
        piece_data,
    }
}

fn fixture(sizes: &[usize]) -> Fixture {
    let names: Vec<String> = (0..sizes.len()).map(|i| format!("GAME_{i}.pkg")).collect();
    build_fixture(sizes, &names, &names)
}

fn cmd() -> Command {
    Command::cargo_bin("pkg-merge").unwrap()
}

fn merge_cmd(fx: &Fixture) -> Command {
    let mut command = cmd();
    command
        .arg(&fx.manifest_path)
        .arg("--output")
        .arg(&fx.out_dir)
        .arg("--no-progress");
    command
}

fn output_path(fx: &Fixture) -> PathBuf {
    fx.out_dir.join("game.pkg")
}

fn part_path(fx: &Fixture) -> PathBuf {
    fx.out_dir.join("game.pkg.part")
}

fn journal_path(fx: &Fixture) -> PathBuf {
    fx.out_dir.join("game.pkg.part.merge.json")
}

fn expected_data(fx: &Fixture) -> Vec<u8> {
    fx.piece_data.iter().flat_map(|data| data.clone()).collect()
}

fn read_manifest(fx: &Fixture) -> Value {
    serde_json::from_slice(&fs::read(&fx.manifest_path).unwrap()).unwrap()
}

fn write_manifest(fx: &Fixture, manifest: &Value) {
    fs::write(
        &fx.manifest_path,
        serde_json::to_vec_pretty(manifest).unwrap(),
    )
    .unwrap();
}

fn serve_once(body: Vec<u8>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut request = [0u8; 8192];
            let _ = stream.read(&mut request);
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(&body);
            let _ = stream.flush();
        }
    });
    port
}

#[test]
fn merge_happy_path() {
    let fx = fixture(&[1024 * 1024, 2 * 1024 * 1024, 512 * 1024]);
    merge_cmd(&fx).assert().success();

    assert_eq!(fs::read(output_path(&fx)).unwrap(), expected_data(&fx));
    assert!(!part_path(&fx).exists());
    assert!(!journal_path(&fx).exists());
    assert!(fx.out_dir.join("game.manifest.json").exists());
}

#[test]
fn corrupt_piece_is_detected() {
    let fx = fixture(&[1024 * 1024, 1024 * 1024, 1024 * 1024]);
    let mut data = fs::read(&fx.piece_paths[1]).unwrap();
    data[123] ^= 0xFF;
    fs::write(&fx.piece_paths[1], data).unwrap();

    merge_cmd(&fx)
        .assert()
        .code(3)
        .stderr(predicates::str::contains("SHA-256"));

    assert!(!output_path(&fx).exists());
    assert!(part_path(&fx).exists());
}

#[test]
fn missing_piece_fails_before_writing() {
    let fx = fixture(&[1024 * 1024, 1024 * 1024]);
    fs::remove_file(&fx.piece_paths[1]).unwrap();

    merge_cmd(&fx)
        .assert()
        .code(3)
        .stderr(predicates::str::contains("faltan piezas"));

    assert!(!output_path(&fx).exists());
    assert!(!part_path(&fx).exists());
}

#[test]
fn check_detects_corruption() {
    let fx = fixture(&[1024 * 1024, 1024 * 1024]);
    cmd()
        .arg(&fx.manifest_path)
        .arg("--check")
        .arg("--no-progress")
        .assert()
        .success();

    let mut data = fs::read(&fx.piece_paths[0]).unwrap();
    data[0] ^= 1;
    fs::write(&fx.piece_paths[0], data).unwrap();

    cmd()
        .arg(&fx.manifest_path)
        .arg("--check")
        .arg("--no-progress")
        .assert()
        .code(3);
}

#[test]
fn check_parallel() {
    let fx = fixture(&[1024 * 1024, 1024 * 1024, 1024 * 1024, 1024 * 1024]);
    cmd()
        .arg(&fx.manifest_path)
        .arg("--check")
        .arg("--jobs")
        .arg("2")
        .arg("--no-progress")
        .assert()
        .success();
}

#[test]
fn in_place_deletes_parts_after_verifying() {
    let fx = fixture(&[1024 * 1024, 2 * 1024 * 1024, 3 * 1024 * 1024]);
    cmd()
        .arg(&fx.manifest_path)
        .arg("--in-place")
        .arg("--delete-parts")
        .arg("--yes")
        .arg("--no-progress")
        .assert()
        .success();

    let final_path = fx.pieces_dir.join("game.pkg");
    assert_eq!(fs::read(&final_path).unwrap(), expected_data(&fx));
    assert!(!fx.piece_paths[0].exists());
    assert!(!fx.piece_paths[1].exists());
    assert!(!fx.piece_paths[2].exists());
    assert!(!fx.pieces_dir.join("game.pkg.part").exists());
}

#[test]
fn in_place_does_not_touch_piece_zero_if_corrupt() {
    let fx = fixture(&[1024 * 1024, 1024 * 1024]);
    let mut data = fs::read(&fx.piece_paths[0]).unwrap();
    data[0] ^= 1;
    fs::write(&fx.piece_paths[0], data).unwrap();

    cmd()
        .arg(&fx.manifest_path)
        .arg("--in-place")
        .arg("--delete-parts")
        .arg("--yes")
        .arg("--no-progress")
        .assert()
        .code(3);

    assert!(fx.piece_paths[0].exists());
    assert!(!fx.pieces_dir.join("game.pkg.part").exists());
}

#[test]
fn delete_parts_requires_confirmation_in_non_tty() {
    let fx = fixture(&[1024 * 1024, 1024 * 1024]);
    cmd()
        .arg(&fx.manifest_path)
        .arg("--in-place")
        .arg("--delete-parts")
        .arg("--no-progress")
        .write_stdin("n\n")
        .assert()
        .code(1);
}

#[test]
fn resumes_after_stop() {
    let fx = fixture(&[1024 * 1024, 1024 * 1024, 1024 * 1024]);
    merge_cmd(&fx)
        .arg("--stop-after")
        .arg("1")
        .assert()
        .code(130);

    assert!(part_path(&fx).exists());
    assert!(journal_path(&fx).exists());
    assert!(!output_path(&fx).exists());

    merge_cmd(&fx).assert().success();
    assert_eq!(fs::read(output_path(&fx)).unwrap(), expected_data(&fx));
    assert!(!journal_path(&fx).exists());
}

#[test]
fn resume_with_verify_final_verifies_file() {
    let fx = fixture(&[1024 * 1024, 1024 * 1024, 1024 * 1024]);
    merge_cmd(&fx)
        .arg("--stop-after")
        .arg("2")
        .assert()
        .code(130);

    merge_cmd(&fx).arg("--verify-final").assert().success();
    assert_eq!(fs::read(output_path(&fx)).unwrap(), expected_data(&fx));
}

#[test]
fn resume_detects_truncated_part() {
    let fx = fixture(&[1024 * 1024, 1024 * 1024, 1024 * 1024]);
    merge_cmd(&fx)
        .arg("--stop-after")
        .arg("1")
        .assert()
        .code(130);

    let file = fs::OpenOptions::new()
        .write(true)
        .open(part_path(&fx))
        .unwrap();
    file.set_len(1).unwrap();
    drop(file);

    merge_cmd(&fx).assert().code(5);
}

#[test]
fn finds_pieces_by_size_when_renamed() {
    let names = vec![
        "z.pkg".to_string(),
        "a.pkg".to_string(),
        "m.pkg".to_string(),
    ];
    let urls = vec![
        "GAME_0.pkg".to_string(),
        "GAME_1.pkg".to_string(),
        "GAME_2.pkg".to_string(),
    ];
    let fx = build_fixture(
        &[1024 * 1024, 2 * 1024 * 1024, 3 * 1024 * 1024],
        &names,
        &urls,
    );
    merge_cmd(&fx).assert().success();
    assert_eq!(fs::read(output_path(&fx)).unwrap(), expected_data(&fx));
}

#[test]
fn finds_pieces_by_hash_when_size_is_ambiguous() {
    let names = vec!["x.pkg".to_string(), "y.pkg".to_string()];
    let urls = vec!["GAME_0.pkg".to_string(), "GAME_1.pkg".to_string()];
    let fx = build_fixture(&[1024 * 1024, 1024 * 1024], &names, &urls);
    merge_cmd(&fx).assert().success();
    assert_eq!(fs::read(output_path(&fx)).unwrap(), expected_data(&fx));
}

#[test]
fn rejects_manifest_with_gap() {
    let fx = fixture(&[1024 * 1024, 1024 * 1024]);
    let mut manifest = read_manifest(&fx);
    let offset = manifest["pieces"][1]["fileOffset"].as_u64().unwrap();
    manifest["pieces"][1]["fileOffset"] = json!(offset + 1);
    write_manifest(&fx, &manifest);

    merge_cmd(&fx)
        .assert()
        .code(2)
        .stderr(predicates::str::contains("hueco o solape"));
}

#[test]
fn package_digest_is_not_compared_by_default() {
    let fx = fixture(&[1024 * 1024, 1024 * 1024]);
    let mut manifest = read_manifest(&fx);
    manifest["packageDigest"] = json!("00".repeat(32));
    write_manifest(&fx, &manifest);

    merge_cmd(&fx).assert().success();

    assert!(output_path(&fx).exists());
    assert!(!part_path(&fx).exists());
}

#[test]
fn verify_final_warns_on_package_digest_mismatch() {
    let fx = fixture(&[1024 * 1024, 1024 * 1024]);
    let mut manifest = read_manifest(&fx);
    manifest["packageDigest"] = json!("00".repeat(32));
    write_manifest(&fx, &manifest);

    merge_cmd(&fx)
        .arg("--verify-final")
        .assert()
        .success()
        .stderr(predicates::str::contains("no coincide"));

    assert!(output_path(&fx).exists());
}

#[test]
fn strict_package_digest_fails() {
    let fx = fixture(&[1024 * 1024, 1024 * 1024]);
    let mut manifest = read_manifest(&fx);
    manifest["packageDigest"] = json!("00".repeat(32));
    write_manifest(&fx, &manifest);

    merge_cmd(&fx)
        .arg("--strict-package-digest")
        .assert()
        .code(4)
        .stderr(predicates::str::contains("packageDigest"));

    assert!(!output_path(&fx).exists());
    assert!(part_path(&fx).exists());
}

#[test]
fn verify_final_without_package_digest_still_verifies_pieces() {
    let fx = fixture(&[1024 * 1024, 1024 * 1024]);
    let mut manifest = read_manifest(&fx);
    manifest.as_object_mut().unwrap().remove("packageDigest");
    write_manifest(&fx, &manifest);

    merge_cmd(&fx)
        .arg("--verify-final")
        .assert()
        .success()
        .stderr(predicates::str::contains("no trae packageDigest"));

    assert!(output_path(&fx).exists());
}

#[test]
fn parts_dir_option_works() {
    let fx = fixture(&[1024 * 1024, 1024 * 1024]);
    let other = fx.dir.path().join("other");
    fs::create_dir_all(&other).unwrap();
    for (index, path) in fx.piece_paths.iter().enumerate() {
        fs::rename(path, other.join(format!("GAME_{index}.pkg"))).unwrap();
    }

    merge_cmd(&fx)
        .arg("--parts-dir")
        .arg(&other)
        .assert()
        .success();
    assert_eq!(fs::read(output_path(&fx)).unwrap(), expected_data(&fx));
}

#[test]
fn finds_manifest_in_folder() {
    let fx = fixture(&[1024 * 1024]);
    cmd()
        .arg(&fx.pieces_dir)
        .arg("--output")
        .arg(&fx.out_dir)
        .arg("--no-progress")
        .assert()
        .success();
    assert_eq!(fs::read(output_path(&fx)).unwrap(), expected_data(&fx));
}

#[test]
fn empty_folder_reports_missing_manifest() {
    let dir = tempfile::tempdir().unwrap();
    cmd()
        .arg(dir.path())
        .arg("--no-progress")
        .assert()
        .code(2)
        .stderr(predicates::str::contains("manifiesto"));
}

#[test]
fn output_name_comes_from_manifest_name() {
    let fx = fixture(&[1024 * 1024]);
    let renamed = fx.pieces_dir.join("my game.json");
    fs::rename(&fx.manifest_path, &renamed).unwrap();

    cmd()
        .arg(&renamed)
        .arg("--output")
        .arg(&fx.out_dir)
        .arg("--no-progress")
        .assert()
        .success();

    let final_path = fx.out_dir.join("my game.pkg");
    assert!(final_path.exists());
    assert_eq!(fs::read(final_path).unwrap(), expected_data(&fx));
}

#[test]
fn single_piece_is_supported() {
    let fx = fixture(&[3 * 1024 * 1024]);
    merge_cmd(&fx).assert().success();
    assert_eq!(fs::read(output_path(&fx)).unwrap(), expected_data(&fx));
}

#[test]
fn refuses_to_overwrite_without_force() {
    let fx = fixture(&[1024 * 1024]);
    fs::write(output_path(&fx), b"old").unwrap();

    merge_cmd(&fx)
        .assert()
        .code(1)
        .stderr(predicates::str::contains("--force"));

    merge_cmd(&fx).arg("--force").assert().success();
    assert_eq!(fs::read(output_path(&fx)).unwrap(), expected_data(&fx));
}

#[test]
fn downloads_missing_piece_directly_to_output() {
    let fx = fixture(&[1024 * 1024, 1024 * 1024]);
    let missing_data = fx.piece_data[1].clone();
    fs::remove_file(&fx.piece_paths[1]).unwrap();

    let port = serve_once(missing_data);
    let mut manifest = read_manifest(&fx);
    manifest["pieces"][1]["url"] = json!(format!("http://127.0.0.1:{port}/GAME_1.pkg"));
    write_manifest(&fx, &manifest);

    merge_cmd(&fx).arg("--download-missing").assert().success();
    assert_eq!(fs::read(output_path(&fx)).unwrap(), expected_data(&fx));
}
