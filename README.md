# Clisage

A small Rust shell AI assistant for macOS and Linux. Install with Homebrew or
build from source with Cargo. SemVer tags identify CI-validated main commits;
the release pipeline publishes prebuilt Homebrew bottles for new releases.

## Install with Homebrew

The repository doubles as a Homebrew tap:

```sh
brew tap alkca/clisage https://github.com/alkca/clisage
brew install alkca/clisage/clisage
```

Homebrew downloads a prebuilt bottle when one is available for your platform,
plus the `pass`/GPG dependencies. Otherwise it installs the Rust build toolchain
and compiles Clisage, which can take several minutes. Older releases predating
automatic bottle publication build from source. Homebrew does not create a GPG key or initialize your
password store: if needed, run `pass init -p clisage YOUR_GPG_KEY_ID` with an existing encryption-capable key,
then `clisage auth login`.

To follow stable SemVer releases:

```sh
brew update
brew upgrade alkca/clisage/clisage
```

If you previously installed with `--HEAD`, switch to stable once after updating
the tap. Homebrew retains the HEAD installation choice during normal upgrades:

```sh
brew update
brew uninstall alkca/clisage/clisage
brew install alkca/clisage/clisage
clisage --version
```

This preserves your Clisage configuration and credential store. Stable installs
report a plain version such as `clisage 0.1.0`. Development builds remain available
with `brew install --HEAD alkca/clisage/clisage` and update using
`brew upgrade --fetch-HEAD alkca/clisage/clisage`.

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
clisage doctor
clisage completions zsh
printf 'ls -l\n' | clisage explain
```

By default, login reads a hidden API key from the terminal, stores it at
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

## Native credential stores and setup checks

`pass` remains the default. You can instead select macOS Keychain or Linux
Secret Service in `config.toml`:

```toml
config_version = 1
model = "gpt-6-luna"
credential_backend = "keychain" # macOS; use "secret-service" on Linux
```

Then run `clisage auth login`. Native login reads a hidden key, writes it through
the platform API, and reads it back to verify storage. The native entry uses
service `clisage/openai/api.openai.com` and account `default`. Linux needs a
running desktop D-Bus session and an unlocked Secret Service provider, such as
GNOME Keyring or a compatible KDE Wallet. macOS may request Keychain access.
There is no plaintext fallback and keys are never passed as process arguments.

Use `clisage auth status --backend keychain` or
`clisage auth logout --backend secret-service` to select a store explicitly.
`--backend` affects only that auth command; change `credential_backend` in the
config to select the store for ordinary requests. Existing keys are not moved
or deleted when you change backends. After logging into the new store and
checking its status, remove the old entry explicitly if desired.

Auth commands remain available when model configuration is broken. If the
configuration cannot be parsed, they announce that they are using `pass`;
`auth --backend` overrides that fallback. Native credential reads and removals
require interactive stderr so unattended jobs cannot open unlock dialogs.
Use `CLISAGE_API_KEY` for automation; it takes precedence over every backend.

Run `clisage doctor` to check config syntax, the selected backend's local
prerequisites, pass recipient metadata, and shell context. It does not read
stored keys, unlock stores, or contact OpenAI. Exit status 1 indicates a setup
problem; availability and provider validity still need `auth status` and an
actual request, respectively. Invalid TOML diagnostics include a line and column
and recovery guidance without printing config values. An empty
`XDG_CONFIG_HOME` falls back to `~/.config`.

## Shell completions

Homebrew installs Bash, Zsh, and Fish completions automatically. For a Cargo
installation, generate the matching script:

```sh
clisage completions bash > clisage.bash
clisage completions zsh > _clisage
clisage completions fish > clisage.fish
```

Source `clisage.bash` in Bash, place `_clisage` in a directory on Zsh's `$fpath`
before running `compinit`, or place `clisage.fish` in
`~/.config/fish/completions/`. Completion generation needs no credentials or
valid config. Fish completions cover the CLI; the Alt-G widget supports Bash
and Zsh.

## Terminal output and editor prompts

Clisage renders Markdown in interactive terminals, including headings, emphasis,
lists, links, tables, and syntax colors for fenced code blocks with a language
label. Highlighting supports shell/Bash, Rust, Python, JavaScript, and JSON, plus
common aliases such as `sh`, `rs`, `py`, and `js`. Other languages remain readable
as plain code. Formatted responses appear after generation completes. Use `--no-markdown`
to see the unformatted response as it streams. Terminal output filters provider-supplied
control characters independently of Markdown, including with `NO_COLOR`. Redirected output keeps
the original bytes and stays unformatted
and streaming by default; `--markdown` explicitly enables formatting there.
Nonempty `NO_COLOR` disables formatting, and `TERM=dumb` disables automatic
formatting. `shell --command-only` always returns a plain command.

Use `--editor` to compose a multiline prompt in `$EDITOR`, falling back to
`$VISUAL`, then `vi`; empty or whitespace-only settings are skipped.
An optional prompt argument pre-fills the temporary file;
this also works with `shell` and `explain`. Editor commands may contain quoted
arguments, such as `EDITOR='code --wait'`; Clisage invokes the editor directly
without shell expansion. Save and close the editor to submit. An empty file or
unsuccessful editor exit cancels the request. `shell` and `explain` can also take their entire request from piped stdin without
a prompt argument. Piped input cannot be combined with
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

The provider and HTTPS endpoint are fixed to OpenAI, binding
stored keys to `api.openai.com`. Unknown config fields are rejected, including
API keys and endpoint overrides. The [Chat Completions API](https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create)
streams responses. The [default model](https://developers.openai.com/api/docs/models/gpt-6-luna)
is configurable; availability depends on your account.
GPT-6 Luna uses reasoning effort `none` for quick shell responses. Other model
overrides use their provider defaults.

Piped input and editor prompts are limited to 1 MiB. Provider responses are
limited to 1 MiB of text, with 64 KiB per SSE line/event and 8 MiB of total stream
data. Incomplete, token-limited, filtered, refused, and empty responses fail with
an error. Unformatted streams may already have emitted partial text; formatted
responses and command-only suggestions are withheld until successful completion.
Command-only suggestions additionally have a 16 KiB limit.

Your explicit prompt and any piped stdin are sent to OpenAI with `store: false`.
Requests also include the OS name, Linux distribution name when available from
`/etc/os-release`, and shell name from `$SHELL`. The Bash/Zsh widgets supply their
actual shell; `CLISAGE_SHELL` can override it for direct CLI use. Only the shell
basename is included, never its filesystem path. Missing shell metadata falls
back to portable POSIX suggestions.
Provider retention policies still apply. Clisage has no telemetry, saved chats,
directory scanning, or shell history collection. Results go to stdout; diagnostics
go to stderr. Shell mode **only displays suggestions**. Review them before use.
Credentials are protected at rest by the selected encrypted store, not against
all same-user access.
Secrets are redacted in diagnostics, but transport libraries may make memory
copies. A broken pipe exits successfully; other runtime errors exit 1 and invalid
CLI usage exits 2. Requests time out after 120 seconds and are not retried.

## Development

```sh
cargo test --locked # Requires Zsh/Bash and pass/GPG; Linux also needs dbus and gnome-keyring.
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
```

Tests use fake credentials and local mock HTTP, without real API keys or paid
requests. CI runs on Linux and macOS. Linux native-store tests start a private
D-Bus session and disposable GNOME keyring; native error handling is also tested
with isolated in-memory stores. Tests do not use your normal credential store.
On Debian/Ubuntu, install `zsh pass gnupg dbus gnome-keyring` before running them.
The tooling suite uses `uv run --no-project --offline python -m unittest discover
-s tests/tooling -v` and requires the repository-pinned release-plz binary on PATH.
Profiles remain follow-up work. Distribution preparation is described below;
SemVer tagging, GitHub Releases, bottle publication, and formula PR creation are
automatic after main CI. PR review and merging remain manual.

License: MIT OR Apache-2.0.

## Build versions

`clisage --version` identifies the source used to compile the binary. A clean
checkout at a stable release tag, such as `v0.2.0`, reports `0.2.0`, even when
Cargo.toml still contains the `0.1.0` bootstrap floor.
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
`release-plz` remains available for manually requested manifest and changelog PRs.
Cargo.toml is a minimum version floor; automatic tags are the release identity.
No extra version-bump commit or release PR is needed for each merge.

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

Every PR squash merge to main, except the formula synchronization described below,
is automatically tagged after its own **main-push** Rust, Quality, PR-title, and Homebrew jobs all succeed on Linux and macOS. The
**Tag SemVer releases** workflow runs after CI/Homebrew completion. It checks
workflow identities, event type, repository, exact commit, and individual job
results; PR/fork checks, skipped jobs, and incomplete runs cannot authorize tags.
Partial retries use the latest attempt of each required job, retaining earlier
successes only for jobs that were not rerun.

With no existing tags, the merge that installs this automation receives
`v0.1.0` from Cargo.toml. Older bootstrap commits are not retroactively released.
Later `feat:` merges advance minor; breaking changes advance major after 1.0
(minor during 0.x); all other Conventional Commit types, including docs and CI,
advance patch. For example: `v0.1.0` → feature `v0.2.0` → fix `v0.2.1` → docs
`v0.2.2`. A manually increased manifest floor is preserved.

Tag creation is serialized. Each run reconciles main commits in order, so
closely spaced merges and out-of-order CI completions cannot skip a version or
release a later commit ahead of an unvalidated earlier one. Existing tags are
immutable; retries are idempotent, and conflicting tags, ambiguous versions,
or gaps in the release history fail closed. The workflow creates lightweight
Git tags, builds and tests Homebrew bottles, publishes them as GitHub Release
assets, and prepares stable Homebrew formula update PRs. It never rewrites main,
moves tags, or publishes crates. Default token permissions remain read-only.
Bottle build jobs receive provenance-signing permissions but no contents-write
access; separate trusted main jobs publish releases and prepare formula PRs.
They never execute PR-run code or download PR-run artifacts with write tokens.

After tagging, the workflow builds and tests bottles for Apple Silicon macOS
and Intel/ARM Linux. It creates a draft GitHub Release, uploads all three bottles,
provenance bundles, and a checksum manifest, verifies the uploaded digests, and
publishes the release only when the complete set is present. Published assets
are never overwritten. This starts with the merge installing automatic bottle
publication; earlier tags are not retroactively published.

The workflow then opens a `chore(homebrew): update clisage to vX.Y.Z`
PR with the immutable tag, full source revision, formula version, and bottle
download URLs and platform checksums from the latest published release. It explicitly
dispatches CI and Homebrew checks because GitHub suppresses events from its own
token. Normal review, required checks, and merging still apply. Stable upgrades
become available when that formula PR merges; a tag alone does not update the tap.
Linux and macOS CI always test the exact PR HEAD. Formula changes additionally
install and test the stable release, verifying that formulas with bottles were
actually installed from a bottle; manual Homebrew dispatches on main also do so.
Unrelated changes avoid rebuilding the previous release.
Stable checks reject a downgrade relative to main, including an obsolete update PR.
Closed update PRs are left closed; reopen them to retry. Interrupted runs retry
without force-pushing or changing existing branch content. Failed release builds
leave the formula unchanged. The next reconciliation retries unpublished tags;
the **Tag SemVer releases** workflow can also be manually dispatched on main.
Incomplete drafts stay unpublished, and conflicting existing assets fail closed
rather than replacing previously uploaded bytes.

Formula synchronization commits are exempt from release tagging to prevent a
release/update loop. The exemption requires the generated title, only a stable
source/version/bottle-metadata change in `Formula/clisage.rb`, and an existing higher release tag
at an ancestor commit. Changes to dependencies, installation logic, tests, or any
other file still receive a release tag.

A failed main check pauses tags at that commit. Fix or rerun its CI, then let a
completion event retry reconciliation. To recover missed completion events or
retry explicitly, dispatch `tag-release.yml` on main.
For a read-only preview from a clean, complete main checkout with authenticated
`gh`, run:

```sh
GITHUB_REPOSITORY=alkca/clisage uv run --no-project --offline python scripts/auto_release.py
```

The script defaults to a dry run. `--apply` is restricted to main-branch Actions
execution. CI tests use local repositories and fake GitHub metadata and never
create real tags. The required workflow/job names are recorded in
`scripts/auto_release.py`; update those gates alongside any CI job renames or
branch-protection changes.

`release-pr.yml` is now a manual-only, optional release-plz planner. Its tag and
GitHub-release options stay disabled because `tag-release.yml` exclusively owns
automatic tags. The planner and checksum remain pinned in
`.github/actions/setup-release-plz/action.yml`. Manually requested bot PRs still
receive explicit CI dispatches. None of these workflows approves or merges PRs.

To install a specific tagged version from source, clone its tag and install:

```sh
git clone --branch v0.1.0 https://github.com/alkca/clisage.git
cd clisage
cargo install --path . --locked
```

Use an existing tag from the repository's Tags list. Homebrew HEAD remains the
latest-development installation path.

## Distribution preparation

The **Distribution artifacts** workflow runs only when manually dispatched on
`main`. It tests and builds native x86-64 and ARM64 binaries on Ubuntu 24.04 and
macOS 15, packages each binary with licenses, this README, and Bash/Zsh/Fish
completions, and uploads archives and SHA-256 checksums as workflow artifacts
(retained for 14 days). macOS builds target macOS 13 or newer; Linux binaries use
the Ubuntu 24.04 glibc baseline. Older Linux systems can build from source.
Archives have normalized timestamps, ownership, and permissions; packaging the
same binary and documentation produces the same archive bytes.

The workflow signs archive provenance using GitHub's Sigstore-backed artifact
attestations and includes an offline verification bundle. After downloading an
archive and its companion files, verify them before extracting:

```sh
shasum -a 256 -c clisage-VERSION-TARGET.tar.gz.sha256
gh attestation verify clisage-VERSION-TARGET.tar.gz --repo alkca/clisage \
  --signer-workflow alkca/clisage/.github/workflows/distribution.yml --source-ref refs/heads/main
# Alternatively, use the included bundle:
gh attestation verify clisage-VERSION-TARGET.tar.gz --repo alkca/clisage \
  --signer-workflow alkca/clisage/.github/workflows/distribution.yml --source-ref refs/heads/main \
  --bundle provenance-TARGET.sigstore.json
```

This signs the archive's provenance; Apple Developer ID signing and notarization
are not configured. The workflow does not create tags or GitHub Releases, push
packages, or publish download URLs. It uses read-only repository contents access
and grants signing permissions only to the manual build job.

The **Publish Homebrew bottles** reusable workflow is called automatically by
**Tag SemVer releases**. It first verifies the release tag is an ancestor of main
with successful required main-push checks, then pins each build to that exact
commit. It requires a clean checkout at the tag, with a version at or above the
manifest floor. Each platform builds and tests a temporary stable formula,
bottles it, and installs and retests it through a temporary bottle formula before
signing provenance. The installation receipt must confirm an actual bottle pour.
The publication job accepts only the complete artifacts from that trusted run.
Bottles and provenance are permanently hosted as assets of the corresponding
GitHub Release; Actions artifacts remain available for 14 days for debugging.

The generated formula PR still requires manual review and merge approval.
After it merges, normal stable `brew upgrade` installs use a compatible bottle
instead of compiling Rust. Intel Macs and HEAD installs continue to build from
source. Intel macOS is excluded from automatic bottles because Homebrew stopped
publishing its dependency bottles in September 2026; a missing Intel dependency
must not block releases for supported platforms.
