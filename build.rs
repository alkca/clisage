mod build_support;

fn main() {
    let root = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let version = build_support::build_version(&root, &std::env::var("CARGO_PKG_VERSION").unwrap());
    println!("cargo::rustc-env=CLISAGE_BUILD_VERSION={version}");
    // Recheck on each Cargo build: refs, tags, untracked files and dirty state
    // can change without touching any source file Cargo previously observed.
    // This intentionally absent path forces the lightweight, local Git checks.
    let check = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap())
        .join("always-check-git-version");
    println!("cargo::rerun-if-changed={}", check.display());
}
