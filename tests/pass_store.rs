use clisage::check_pass_store;

#[test]
fn missing_store_reports_initialization_guidance() {
    let root = tempfile::tempdir().unwrap();
    let error = check_pass_store(root.path(), false)
        .unwrap_err()
        .to_string();
    assert!(error.contains("pass init -p clisage YOUR_GPG_KEY_ID"));
    assert!(!root.path().join("clisage").exists());
}

#[test]
fn root_or_clisage_scoped_recipient_configuration_is_supported() {
    for scope in [
        "",
        "clisage",
        "clisage/openai",
        "clisage/openai/api.openai.com",
    ] {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join(scope);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join(".gpg-id"),
            "# test recipient\nfake-test-recipient\n",
        )
        .unwrap();
        check_pass_store(root.path(), false).unwrap();
    }
}

#[test]
fn closest_recipient_configuration_must_be_nonempty() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join(".gpg-id"), "fake-test-recipient").unwrap();
    std::fs::create_dir(root.path().join("clisage")).unwrap();
    std::fs::write(root.path().join("clisage/.gpg-id"), "# no recipients\n\n").unwrap();
    assert!(check_pass_store(root.path(), false).is_err());
}

#[test]
fn explicit_pass_recipient_override_does_not_require_metadata_files() {
    let root = tempfile::tempdir().unwrap();
    check_pass_store(root.path(), true).unwrap();
}
