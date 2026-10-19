//! Integration tests covering all [`FixtureLoaderError`] / [`FixtureError`]
//! variants when loading and validating fixture files and directories.

use std::path::{Path, PathBuf};

use canary_fixtures::{
    load_directory, parse_fixture_file, parse_fixture_str, unknown_body_keys, validate,
    FixtureError, FixtureLoaderError, MAX_FILE_BYTES,
};

fn unique_temp_dir(prefix: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut path = std::env::temp_dir();
    path.push(format!(
        "canary-loader-errors-{prefix}-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn valid_header(id: &str) -> String {
    format!(
        "id = \"{id}\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"test fixture\"\n"
    )
}

#[test]
fn missing_file_returns_read_variant_with_filename() {
    let dir = unique_temp_dir("missing-file");
    let missing_path = dir.join("does_not_exist.toml");

    let err = parse_fixture_file(&missing_path).unwrap_err();
    match &err {
        FixtureLoaderError::Read { path, .. } => assert_eq!(path, &missing_path),
        other => panic!("expected Read variant, got: {other:?}"),
    }
    assert!(
        err.to_string().contains("does_not_exist.toml"),
        "error message must contain filename: {err}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn missing_directory_returns_read_dir_variant_with_path() {
    let dir = unique_temp_dir("missing-dir");
    let missing_dir = dir.join("nonexistent_subdir");

    let err = load_directory(&missing_dir).unwrap_err();
    match &err {
        FixtureLoaderError::ReadDir { path, .. } => assert_eq!(path, &missing_dir),
        other => panic!("expected ReadDir variant, got: {other:?}"),
    }
    assert!(err.to_string().contains("nonexistent_subdir"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn invalid_toml_syntax_returns_parse_variant_with_filename_and_line_number() {
    let dir = unique_temp_dir("invalid-toml");
    let file_path = dir.join("broken_syntax.toml");
    std::fs::write(
        &file_path,
        "id = \"p28-xdr-001\"\nprotocol = 28\nsurface = [[[invalid toml\n",
    )
    .unwrap();

    let err = load_directory(&dir).unwrap_err();
    match &err {
        FixtureLoaderError::Parse { path, .. } => assert_eq!(path, &file_path),
        other => panic!("expected Parse variant, got: {other:?}"),
    }
    let rendered = err.to_string();
    assert!(
        rendered.contains("broken_syntax.toml"),
        "expected filename in error: {rendered}"
    );
    assert!(
        rendered.contains("line 3"),
        "expected line number in error: {rendered}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn duplicate_fixture_id_returns_duplicate_id_variant_naming_both_files() {
    let dir = unique_temp_dir("duplicate-id");
    let first_path = dir.join("a_first.toml");
    let second_path = dir.join("b_second.toml");
    std::fs::write(&first_path, valid_header("p28-dup-001")).unwrap();
    std::fs::write(&second_path, valid_header("p28-dup-001")).unwrap();

    let loaded = load_directory(&dir).unwrap();
    let err = validate(&loaded).unwrap_err();
    match &err {
        FixtureLoaderError::DuplicateId { id, first, second } => {
            assert_eq!(id, "p28-dup-001");
            assert_eq!(first, &first_path);
            assert_eq!(second, &second_path);
        }
        other => panic!("expected DuplicateId variant, got: {other:?}"),
    }
    let rendered = err.to_string();
    assert!(rendered.contains("p28-dup-001"), "{rendered}");
    assert!(rendered.contains("a_first.toml"), "{rendered}");
    assert!(rendered.contains("b_second.toml"), "{rendered}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn oversized_fixture_exceeding_1_mib_fails_with_file_too_large() {
    let dir = unique_temp_dir("oversized");
    let file_path = dir.join("oversized_fixture.toml");
    let padding = "x".repeat((MAX_FILE_BYTES as usize) + 1024);
    let content = format!(
        "id = \"p28-oversized\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"{padding}\"\n"
    );
    std::fs::write(&file_path, content).unwrap();

    let err = load_directory(&dir).unwrap_err();
    match &err {
        FixtureLoaderError::FileTooLarge { path, size, limit } => {
            assert_eq!(path, &file_path);
            assert!(*size > MAX_FILE_BYTES);
            assert_eq!(*limit, MAX_FILE_BYTES);
        }
        other => panic!("expected FileTooLarge variant, got: {other:?}"),
    }
    assert!(
        err.to_string().contains("oversized_fixture.toml"),
        "expected filename in error: {err}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unknown_verification_keys_and_tables_are_rejected_with_filename() {
    let raw = format!(
        "{}\n[verification]\nunknown_table_key = \"unexpected\"\n",
        valid_header("p28-bad-verif")
    );
    let err = parse_fixture_str(&raw, Path::new("fixtures/bad_verification.toml")).unwrap_err();
    match &err {
        FixtureLoaderError::InvalidVerification {
            source_path,
            reason,
        } => {
            assert_eq!(source_path, Path::new("fixtures/bad_verification.toml"));
            assert!(reason.contains("unknown field"), "{reason}");
        }
        other => panic!("expected InvalidVerification variant, got: {other:?}"),
    }
    assert!(err.to_string().contains("bad_verification.toml"), "{err}");

    // Also verify surface-level unknown table detection via `unknown_body_keys`.
    let with_unknown_table = format!(
        "{}\n[unexpected_table]\nfoo = \"bar\"\n",
        valid_header("p28-unknown-table")
    );
    let loaded =
        parse_fixture_str(&with_unknown_table, Path::new("fixtures/unknown.toml")).unwrap();
    let body_table = loaded.body.as_table().unwrap();
    let msg = unknown_body_keys(body_table, &["type", "kind", "value_base64"]).unwrap();
    assert!(msg.contains("\"unexpected_table\""), "{msg}");
}

#[test]
fn unsafe_reference_and_missing_referenced_file_and_symlink_variants_match() {
    let unsafe_toml = format!(
        "{}input_file = \"../escape.bin\"\n",
        valid_header("p28-unsafe-ref")
    );
    let err = parse_fixture_str(&unsafe_toml, Path::new("fixtures/unsafe.toml")).unwrap_err();
    assert!(matches!(
        err,
        FixtureError::UnsafeReference {
            field: "input_file",
            ..
        }
    ));
    assert!(err.to_string().contains("unsafe.toml"), "{err}");

    let dir = unique_temp_dir("missing-ref");
    let fixture_path = dir.join("missing_ref.toml");
    std::fs::write(
        &fixture_path,
        format!(
            "{}input_file = \"missing_payload.bin\"\n",
            valid_header("p28-missing-ref")
        ),
    )
    .unwrap();
    let loaded = load_directory(&dir).unwrap();
    let err = validate(&loaded).unwrap_err();
    assert!(matches!(
        err,
        FixtureError::MissingReferencedFile { kind: "input", .. }
    ));
    assert!(err.to_string().contains("missing_ref.toml"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);

    let sym = FixtureError::SymbolicLink {
        path: PathBuf::from("fixtures/link.toml"),
    };
    assert!(sym.to_string().contains("link.toml"));
}
