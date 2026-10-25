# Changelog

All notable changes to Clisage are recorded here. Release PRs maintain this file
from Conventional Commits. No version has been released yet.

## [Unreleased]

## [0.1.0](https://github.com/alkca/clisage/releases/tag/v0.1.0) - 2026-10-25

### Added

- add Git-derived build versions and animated progress ([#4](https://github.com/alkca/clisage/pull/4))
- add platform-aware prompts and terminal enhancements ([#3](https://github.com/alkca/clisage/pull/3))
- add secure Clisage CLI with enforced quality gates ([#1](https://github.com/alkca/clisage/pull/1))

### Other

- Initial commit

### Added

- Automatic Git-derived SemVer build identities in `clisage --version`, including
  commit counts, source revision, dirty state, and explicit archive/shallow-clone
  markers, without modifying manifests or creating releases during builds.
- OS, Linux distribution, and shell awareness in requests, including the actual
  shell used by the Bash and Zsh widgets.
- Terminal Markdown rendering and syntax highlighting for shell/Bash, Rust,
  Python, JavaScript, and JSON; `--markdown` and `--no-markdown` output controls.
- Multiline prompt composition with `--editor`, using `$EDITOR`, `$VISUAL`, or
  `vi`, with empty settings skipped and quoted editor arguments supported.
- A Rust-owned animated thinking indicator on interactive stderr during CLI and
  shell-widget requests, with a configurable Nerd Font badge
  (`CLISAGE_THINKING_ICON`) and clean stdout for command suggestions and pipelines.
- Streaming prompts, piped input, shell suggestions, and command explanations.
- Encrypted credentials through pass/GPG, with initialization checked before login.
- Opt-in Alt-G integration for Zsh and Bash.
- Homebrew source installation on Linux and macOS.
- GPT-6 Luna as the default model.
