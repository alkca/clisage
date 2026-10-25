# Clisage

A small Rust shell AI assistant for macOS and Linux. Early development: install
from source with Homebrew or Cargo. No tagged release or prebuilt package yet.

## Install with Homebrew

The repository doubles as a Homebrew tap:

```sh
brew tap alkca/clisage https://github.com/alkca/clisage
brew install --HEAD alkca/clisage/clisage
```

Homebrew installs the Rust build toolchain and `pass`/GPG dependencies, then
compiles Clisage. The first install can take several minutes. There are no
prebuilt bottles yet. Homebrew does not create a GPG key or initialize your
password store: if needed, run `pass init -p clisage YOUR_GPG_KEY_ID` with an existing encryption-capable key,
then `clisage auth login`.

To update this development installation later:

```sh
brew upgrade --fetch-HEAD alkca/clisage/clisage
```

## Try it

For a Cargo installation, use a current stable Rust toolchain. Persistent credentials additionally
require [`pass`](https://www.passwordstore.org/), GPG, an initialized password
store (`pass init -p clisage YOUR_GPG_KEY_ID`), and working pinentry.

```sh
cargo install --path . --locked
clisage auth login
clisage 'Explain Unix pipes'
clisage shell 'Find files larger than 100 MB'
clisage explain 'find . -type f -size +100M'
clisage --editor
clisage shell --editor 'Find large files'
clisage --no-markdown 'Explain Unix pipes'
git diff | clisage 'Write a commit message'
clisage auth status
clisage config show
clisage auth logout
```

Login reads a hidden API key from the terminal, stores it at
`clisage/openai/api.openai.com/default` in `pass`, then reads it back to verify
storage. Login checks recipient configuration before requesting an API key.
Both a root `.gpg-id` and Clisage-scoped recipient configuration are supported;
`PASSWORD_STORE_DIR` and `PASSWORD_STORE_KEY` follow `pass` conventions.
Clisage never initializes your GPG store automatically. Noninteractive
login fails without reading a key from stdin. Interactive first use offers the
same login when there is no entry. A locked, missing or unavailable store
produces actionable guidance and never falls back to a plaintext file.

For automation, supply `CLISAGE_API_KEY` through your CI secret manager. It takes
precedence over `pass` and is never persisted. No API-key command-line flag exists.
`auth status` checks local availability, not whether OpenAI accepts the key.
`auth logout` removes the stored key; unset the environment variable separately.

## Terminal output and editor prompts

Clisage renders Markdown in interactive terminals, including headings, emphasis,
lists, links, tables, and syntax colors for fenced code blocks with a language
label. Highlighting supports shell/Bash, Rust, Python, JavaScript, and JSON, plus
common aliases such as `sh`, `rs`, `py`, and `js`. Other languages remain readable
as plain code. Formatted responses appear after generation completes. Use `--no-markdown`
to see the original response as it streams. Redirected output stays unformatted
and streaming by default; `--markdown` explicitly enables formatting there.
Nonempty `NO_COLOR` disables formatting, and `TERM=dumb` disables automatic
formatting. `shell --command-only` always returns a plain command.

Use `--editor` to compose a multiline prompt in `$EDITOR`, falling back to
`$VISUAL`, then `vi`; empty or whitespace-only settings are skipped.
An optional prompt argument pre-fills the temporary file;
this also works with `shell` and `explain`. Editor commands may contain quoted
arguments, such as `EDITOR='code --wait'`; Clisage invokes the editor directly
without shell expansion. Save and close the editor to submit. An empty file or
unsuccessful editor exit cancels the request. Piped input cannot be combined with
`--editor`. The temporary prompt file is private and removed when Clisage exits
normally; editor-created backups follow your editor settings.

## Alt-G shell integration

After installing Clisage and running `clisage auth login`, enable the integration
in your current shell. To keep it enabled, add the matching line near the end of
your shell config, after other plugins that set key bindings.

Zsh (`~/.zshrc`):

```zsh
eval "$(clisage integration zsh)"
```

Bash 4+ with Readline (`~/.bashrc`):

```bash
eval "$(clisage integration bash)"
```

Type a request such as `find files larger than 100 MB` at the shell prompt and
press **Alt-G**. Clisage replaces the line with a command suggestion. Review or
edit it, then press Enter yourself to execute it. It never accepts the line or
evaluates model output. Empty input does nothing; a failed or cancelled request
keeps the original text and cursor position. Responses are buffered until
complete; multiline, Markdown-fenced, and control-character output is rejected.

While a request is running, Rust displays ** Clisage: thinking... ⠋** on
interactive stderr. The final character animates ten times per second. It clears
before the first streamed text, or when a buffered suggestion or formatted
response is ready, and before errors. Animation starts after credential reads,
GPG unlocks, and login prompts finish so it cannot overwrite a pinentry prompt.
This works in both direct
CLI use and the shell widgets, including widgets bound to a different shortcut.
Redirected stderr and `TERM=dumb` disable the indicator; ordinary redirected
stdout also disables it, while `shell --command-only` keeps stderr progress for
the widgets. The indicator never goes to stdout.

Use a Nerd Font in your terminal for the badge glyph. You can substitute an icon
or ASCII text with `export CLISAGE_THINKING_ICON='...'`, or set it to an empty
string to show only the animated spinner and message. Reload the integration
after upgrading so older widgets do not also display their static message.

The integration binds Alt-g and Alt-Shift-g in Emacs and Vi insert modes, without
changing your editing mode. Enabling it replaces any existing bindings on those
keys, including a ShellGPT binding. Your terminal must send Alt/Option as Meta
(Escape); **Escape followed by g** also works. On macOS, enable your terminal's
Option-as-Meta setting and use Zsh or an updated Bash; Apple's stock Bash 3.2
does not support this integration.

The `eval` above loads only Clisage's bundled integration script. Clisage does not
edit your shell config. Only the current input line is sent when you press the
hotkey, so avoid triggering it on a line containing secrets. No shell history or
directory contents are collected. Authenticate before using the widget; it does
not run API-key onboarding while editing a command. GPG may still ask you to
unlock your stored key. You can also request a plain
command with `clisage shell --command-only 'find large files'`.

## Configuration and privacy

Optional config: `$XDG_CONFIG_HOME/clisage/config.toml`, otherwise
`~/.config/clisage/config.toml` (including on macOS). `config show` prints defaults
without writing files. Create this file yourself to change the model:

```toml
config_version = 1
model = "gpt-6-luna"
credential_backend = "pass"
```

The provider and HTTPS endpoint are fixed to OpenAI in this first slice, binding
stored keys to `api.openai.com`. Unknown config fields are rejected, including
API keys and endpoint overrides. The [Chat Completions API](https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create)
streams responses. The [default model](https://developers.openai.com/api/docs/models/gpt-6-luna)
is configurable; availability depends on your account.
GPT-6 Luna uses reasoning effort `none` for quick shell responses. Other model
overrides use their provider defaults.

Your explicit prompt and any piped stdin are sent to OpenAI with `store: false`.
Requests also include the OS name, Linux distribution name when available from
`/etc/os-release`, and shell name from `$SHELL`. The Bash/Zsh widgets supply their
actual shell; `CLISAGE_SHELL` can override it for direct CLI use. Only the shell
basename is included, never its filesystem path. Missing shell metadata falls
back to portable POSIX suggestions.
Provider retention policies still apply. Clisage has no telemetry, saved chats,
directory scanning, or shell history collection. Results go to stdout; diagnostics
go to stderr. Shell mode **only displays suggestions**. Review them before use.
Credentials are protected at rest by GPG, not against all same-user access.
Secrets are redacted in diagnostics, but transport libraries may make memory
copies. A broken pipe exits successfully; other runtime errors exit 1 and invalid
CLI usage exits 2. Requests time out after 120 seconds and are not retried.

## Development

```sh
cargo test --locked # Requires Zsh and Bash for integration tests.
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
```

Tests use fake credentials and local mock HTTP, without real API keys or paid
requests. CI runs on Linux and macOS. Native Apple Keychain / Linux Secret Service,
profiles, completions, signed release artifacts, and prebuilt Homebrew bottles are
follow-up work. No release or merge is performed by the bootstrap work.

License: MIT OR Apache-2.0.

## Build versions

`clisage --version` identifies the source used to compile the binary. A clean
checkout at its matching release tag, such as `v0.1.0`, reports `0.1.0`.
Development builds report a SemVer prerelease such as
`0.2.0-dev.7+g70781bad2390`: the next release target, number of commits since the
last reachable stable release tag, and a 12-character commit hash. Rebuilding the
same clean commit produces the same version. Uncommitted changes add `.dirty`.
Before the first release tag, the target comes from Cargo.toml and the count
covers the available commit history.

The target follows Conventional Commits: `fix:` advances patch, `feat:` advances
minor, and breaking changes advance major after 1.0 (minor during 0.x). Other
development-only commits use the next patch target. A higher manifest version
is preserved. This describes a development build; it does not create a release.
`release-plz` continues to prepare reviewed manifest and changelog updates.

The local build script rechecks Git on each Cargo build to detect tag changes,
empty commits, and dirty state without changing source files or contacting the
network. Archives or builds without usable Git metadata report
`0.1.0-dev.0+unknown` (using the manifest's version). Shallow clones add `.shallow`
because history and the release target may be incomplete. CI fetches full
history, and Homebrew CI verifies that the installed binary identifies the exact
tested commit. No timestamps or per-machine counters enter the version.

## Quality gates and release planning

Every push and PR runs Rust tests, formatting, and Clippy on Linux and macOS.
Credential lifecycle tests generate a disposable GPG encryption key and use a
fake API credential; they never access your keyring or call OpenAI. Homebrew CI
also runs for every change and builds the exact checked-out commit, including
PR merge commits, rather than downloading a moving development branch. It checks
the installed revision before package smoke tests and formula linting.

The Quality job scans complete Git history with Gitleaks, audits Cargo.lock
against RustSec, checks workflows with actionlint and Bash with ShellCheck, and
tests the CI helpers and actual release planner in temporary repositories.
Main requires the Linux/macOS Rust and Homebrew checks, Quality, and PR title
checks, with up-to-date branches and resolved conversations. The configuration
is recorded in `.github/branch-protection.json`. A sole maintainer can merge a
reviewed PR after checks; another human approval is not required. Copilot review
comments must still be inspected and addressed after CI, as documented in
AGENTS.md.

PR titles follow Conventional Commits and become squash commit titles:

- `fix: ...` proposes a patch version.
- `feat: ...` proposes a minor version, including during 0.x development.
- `feat!: ...` or another breaking-change commit proposes a major version after
  1.0; during 0.x development it proposes the next minor version.

After changes merge to main, the Release PR workflow uses release-plz to prepare
a draft PR updating Cargo.toml, Cargo.lock, and CHANGELOG.md. The package is
versioned from Git tags, with no crates.io publication. No existing tag means
an initial release proposal at the manifest's version. The planner and its
checksum are pinned in `.github/actions/setup-release-plz/action.yml`.

This workflow only prepares PRs. It never merges, creates a tag, publishes a
GitHub release, or ships binaries. Actual release/tag creation needs explicit
maintainer authorization; that authorized tag then becomes the baseline for the
next version proposal. Version/changelog edits alone do not establish a released
version. Bot-created release PRs receive explicit CI dispatches because GitHub
suppresses automatic PR events made with GITHUB_TOKEN; no personal token is
required. Existing bootstrap commits are retained unchanged. Future squash PR
titles supply semantic version information.

To activate release PR creation, a maintainer must explicitly enable Settings →
Actions → General → **Allow GitHub Actions to create and approve pull requests**.
GitHub bundles those capabilities into one setting, so enabling it requires
separate approval. Default workflow token permissions stay read-only; only the
trusted main-branch release-planning job receives the write permissions it needs.
The workflow never approves reviews or merges PRs.
