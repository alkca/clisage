#[path = "../build_support.rs"]
mod build_support;

use std::{path::Path, process::Command};

fn git(path: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .current_dir(path)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Test Fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "Test Fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
        .env_remove("GIT_CONFIG")
        .env_remove("GIT_CONFIG_COUNT")
        .env_remove("GIT_CONFIG_PARAMETERS")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "fixture git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "--initial-branch=main"]);
    git(dir.path(), &["config", "user.name", "Test Fixture"]);
    git(
        dir.path(),
        &["config", "user.email", "fixture@example.invalid"],
    );
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname='fixture'\nversion='0.1.0'\n",
    )
    .unwrap();
    git(dir.path(), &["add", "Cargo.toml"]);
    git(dir.path(), &["commit", "-m", "chore: baseline"]);
    dir
}

#[test]
fn untagged_builds_are_identifiable_deterministic_and_do_not_mutate_sources() {
    let dir = fixture();
    let sha = git(dir.path(), &["rev-parse", "--short=12", "HEAD"]);
    let expected = format!("0.1.0-dev.1+g{sha}");
    assert_eq!(build_support::build_version(dir.path(), "0.1.0"), expected);
    assert_eq!(build_support::build_version(dir.path(), "0.1.0"), expected);
    assert!(git(dir.path(), &["status", "--porcelain"]).is_empty());
    std::fs::write(dir.path().join("new-file"), "fixture").unwrap();
    assert_eq!(
        build_support::build_version(dir.path(), "0.1.0"),
        format!("{expected}.dirty")
    );
    git(dir.path(), &["add", "new-file"]);
    assert!(build_support::build_version(dir.path(), "0.1.0").ends_with(".dirty"));
}

#[test]
fn clean_matching_release_tags_are_plain_versions_but_dirty_builds_are_not_releases() {
    let dir = fixture();
    git(dir.path(), &["tag", "v0.1.0"]);
    assert_eq!(build_support::build_version(dir.path(), "0.1.0"), "0.1.0");
    std::fs::write(dir.path().join("Cargo.toml"), "changed fixture").unwrap();
    let version = build_support::build_version(dir.path(), "0.1.0");
    assert!(version.starts_with("0.1.1-dev.0+g"), "{version}");
    assert!(version.ends_with(".dirty"));
}

#[test]
fn conventional_commits_choose_the_next_semver_target() {
    for (baseline, message, next) in [
        ("0.1.0", "fix: repair output", "0.1.1"),
        ("0.1.0", "feat(shell): add output", "0.2.0"),
        ("0.1.0", "refactor!: change interface", "0.2.0"),
        ("1.2.3", "feat: add output", "1.3.0"),
        (
            "1.2.3",
            "fix: change output\n\nBREAKING CHANGE: new interface",
            "2.0.0",
        ),
        ("1.2.3", "docs: describe usage", "1.2.4"),
        (
            "1.2.3",
            "fix: repair output\n\nExample: feat: not a commit header",
            "1.2.4",
        ),
    ] {
        let dir = fixture();
        git(dir.path(), &["tag", &format!("v{baseline}")]);
        git(dir.path(), &["commit", "--allow-empty", "-m", message]);
        let version = build_support::build_version(dir.path(), baseline);
        assert!(
            version.starts_with(&format!("{next}-dev.1+g")),
            "{message}: {version}"
        );
    }
}

#[test]
fn the_highest_change_wins_and_manifest_version_is_never_downgraded() {
    let dir = fixture();
    git(dir.path(), &["tag", "v0.1.0"]);
    git(dir.path(), &["tag", "vnot-semver"]);
    git(
        dir.path(),
        &["commit", "--allow-empty", "-m", "feat: add output"],
    );
    git(
        dir.path(),
        &["commit", "--allow-empty", "-m", "fix: repair output"],
    );
    assert!(build_support::build_version(dir.path(), "0.1.0").starts_with("0.2.0-dev.2+g"));
    assert!(build_support::build_version(dir.path(), "0.3.0").starts_with("0.3.0-dev.2+g"));
}

#[test]
fn archives_and_shallow_clones_are_explicit_and_linked_worktrees_are_supported() {
    let archive = tempfile::tempdir().unwrap();
    assert_eq!(
        build_support::build_version(archive.path(), "0.1.0"),
        "0.1.0-dev.0+unknown"
    );
    let dir = fixture();
    git(dir.path(), &["tag", "v0.1.0"]);
    git(
        dir.path(),
        &["commit", "--allow-empty", "-m", "feat: add output"],
    );
    let linked = dir.path().join("linked");
    git(
        dir.path(),
        &[
            "worktree",
            "add",
            "--detach",
            linked.to_str().unwrap(),
            "HEAD",
        ],
    );
    // Exclude the test's nested worktree from the original fixture's dirty state.
    std::fs::write(dir.path().join(".git/info/exclude"), "linked/\n").unwrap();
    assert_eq!(
        build_support::build_version(&linked, "0.1.0"),
        build_support::build_version(dir.path(), "0.1.0")
    );
    let shallow = archive.path().join("shallow");
    git(
        archive.path(),
        &[
            "clone",
            "--depth=1",
            &format!("file://{}", dir.path().display()),
            shallow.to_str().unwrap(),
        ],
    );
    assert!(build_support::build_version(&shallow, "0.1.0").ends_with(".shallow"));
}

#[test]
fn cargo_rechecks_tags_commits_and_dirty_state_without_source_changes() {
    let dir = fixture();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for file in ["build.rs", "build_support.rs"] {
        std::fs::copy(root.join(file), dir.path().join(file)).unwrap();
    }
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname='version-fixture'\nversion='0.1.0'\nedition='2024'\n[build-dependencies]\nsemver='1'\n").unwrap();
    std::fs::write(
        dir.path().join("src/main.rs"),
        "fn main() { println!(\"{}\", env!(\"CLISAGE_BUILD_VERSION\")); }\n",
    )
    .unwrap();
    std::fs::write(dir.path().join(".gitignore"), "target/\n").unwrap();
    std::fs::write(dir.path().join("notes"), "fixture notes").unwrap();
    let cargo = |args: &[&str]| {
        let output = Command::new(env!("CARGO"))
            .args(args)
            .current_dir(dir.path())
            .env("CARGO_TARGET_DIR", dir.path().join("target"))
            .env("CARGO_NET_OFFLINE", "true")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "fixture Cargo failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    };
    cargo(&["generate-lockfile", "--offline"]);
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "chore: package fixture"]);
    let first = cargo(&["run", "--quiet", "--locked", "--offline"]);
    assert!(first.starts_with("0.1.0-dev.2+g"), "{first}");
    git(dir.path(), &["tag", "v0.1.0"]);
    assert_eq!(cargo(&["run", "--quiet", "--locked", "--offline"]), "0.1.0");
    git(
        dir.path(),
        &["commit", "--allow-empty", "-m", "feat: new behavior"],
    );
    let next = cargo(&["run", "--quiet", "--locked", "--offline"]);
    assert!(next.starts_with("0.2.0-dev.1+g"), "{next}");
    assert_eq!(cargo(&["run", "--quiet", "--locked", "--offline"]), next);
    std::fs::write(dir.path().join("notes"), "changed notes").unwrap();
    assert_eq!(
        cargo(&["run", "--quiet", "--locked", "--offline"]),
        format!("{next}.dirty")
    );
}
