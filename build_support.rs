use semver::{BuildMetadata, Prerelease, Version};
use std::{path::Path, process::Command};

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(["-c", "core.fsmonitor=false"])
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env_remove("GIT_CONFIG")
        .env_remove("GIT_CONFIG_COUNT")
        .env_remove("GIT_CONFIG_PARAMETERS")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8(output.stdout).ok())?
        .map(|text| text.trim().to_owned())
}

fn development(mut target: Version, count: &str, metadata: &str) -> String {
    let pre = if target.pre.is_empty() {
        format!("dev.{count}")
    } else {
        format!("{}.dev.{count}", target.pre)
    };
    target.pre = Prerelease::new(&pre).expect("generated prerelease is valid");
    target.build = BuildMetadata::new(metadata).expect("generated build metadata is valid");
    target.to_string()
}

fn next_target(base: &Version, commits: &str) -> Option<Version> {
    let mut feature = false;
    let mut breaking = false;
    for message in commits.split('\0') {
        let message = message.trim();
        let Some(header) = message.lines().next() else {
            continue;
        };
        let Some((prefix, description)) = header.split_once(':') else {
            continue;
        };
        if description.trim().is_empty() {
            continue;
        }
        let kind = prefix.trim_end_matches('!').split('(').next().unwrap_or("");
        if kind.is_empty() || !kind.chars().all(|c| c.is_ascii_alphabetic()) {
            continue;
        }
        feature |= kind == "feat";
        breaking |= prefix.ends_with('!')
            || message.lines().skip(1).any(|line| {
                line.starts_with("BREAKING CHANGE:") || line.starts_with("BREAKING-CHANGE:")
            });
    }
    if breaking && base.major > 0 {
        Some(Version::new(base.major.checked_add(1)?, 0, 0))
    } else if feature || breaking {
        Some(Version::new(base.major, base.minor.checked_add(1)?, 0))
    } else {
        // Even documentation-only development commits must sort after their
        // previous release, without claiming that a patch has been released.
        Some(Version::new(
            base.major,
            base.minor,
            base.patch.checked_add(1)?,
        ))
    }
}

/// Derive a read-only build identity. Git and tags are inspected locally; this
/// never fetches, updates manifests, creates tags, or publishes releases.
pub fn build_version(root: &Path, manifest: &str) -> String {
    let manifest = Version::parse(manifest).expect("Cargo supplied a valid package version");
    let unknown = || development(manifest.clone(), "0", "unknown");
    let derive = || -> Option<String> {
        // Avoid mistaking an archive nested in another repository for a clone.
        git(root, &["ls-files", "--error-unmatch", "--", "Cargo.toml"])?;
        let sha = git(root, &["rev-parse", "--verify", "HEAD"])?;
        if !(sha.len() == 40 || sha.len() == 64) || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let dirty = !git(root, &["status", "--porcelain", "--untracked-files=normal"])?.is_empty();
        let exact = git(
            root,
            &[
                "rev-parse",
                "--verify",
                &format!("refs/tags/v{manifest}^{{commit}}"),
            ],
        );
        if !dirty && exact.as_deref() == Some(&sha) {
            return Some(manifest.to_string());
        }
        let tags = git(root, &["tag", "--merged", "HEAD", "--list", "v*"])?;
        let baseline = tags
            .lines()
            .filter_map(|tag| {
                let version = Version::parse(tag.strip_prefix('v')?).ok()?;
                version.pre.is_empty().then_some((tag, version))
            })
            .max_by(|a, b| a.1.cmp_precedence(&b.1));
        let (target, range) = if let Some((tag, version)) = baseline {
            let range = format!("{tag}..HEAD");
            let commits = git(root, &["log", "--format=%B%x00", &range, "--"])?;
            let next = next_target(&version, &commits)?;
            let target = if manifest.cmp_precedence(&next).is_gt() {
                manifest.clone()
            } else {
                next
            };
            (target, range)
        } else {
            (manifest.clone(), "HEAD".to_owned())
        };
        let count = git(root, &["rev-list", "--count", &range, "--"])?;
        if count.is_empty() || !count.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let mut metadata = format!("g{}", &sha[..12]);
        if dirty {
            metadata.push_str(".dirty");
        }
        if git(root, &["rev-parse", "--is-shallow-repository"])? == "true" {
            metadata.push_str(".shallow");
        }
        Some(development(target, &count, &metadata))
    };
    derive().unwrap_or_else(unknown)
}
