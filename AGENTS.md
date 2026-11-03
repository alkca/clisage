# Contribution policy

Inspect the branch and worktree before editing. On a clean main branch, run
`git pull --ff-only` and create a descriptive feature branch. Do not move or
discard existing changes on a dirty main branch.

Write behavioral tests before production changes and establish the expected
failure when practical. Run cargo test --locked, cargo fmt --check, and cargo
clippy --all-targets --locked -- -D warnings before publishing changes.

Never commit secrets, private personal information, local machine paths, or real
credential fixtures. Public GitHub identity is permitted. Use clearly fake test
values. Scan staged changes and Git history with gitleaks before pushing. Never
print credentials or raw credential-store/provider error bodies.

Do not merge or publish releases without explicit authorization; automatic
SemVer tags after successful main CI are authorized by the policy below.
For each PR managed, inspect GitHub Copilot review comments after CI completes,
address applicable recommendations, validate and push follow-up changes. Record
why any recommendation
does not apply in the PR workflow summary.

CI and release policy:
- Use Conventional Commit PR titles; squash merges preserve the semantic title.
- Keep Homebrew tests pinned to the checkout SHA and enabled on every behavioral
  PR. Only verified, generated stable-source/bottle-metadata updates may reuse
  their release's successful source checks and bottle pour tests.
- Run `uv run --no-project --offline python -m unittest discover -s tests/tooling -v`
  with release-plz available, plus actionlint, ShellCheck, and cargo audit.
- Tag every main PR squash commit after its own required main-push CI and
  Homebrew jobs succeed. Use immutable, monotonically increasing SemVer tags;
  serialize and reconcile merges in order. The first tag is the manifest
  bootstrap version; features bump minor, breaking changes bump major (minor
  during 0.x), and other PRs bump patch. Never move or overwrite existing tags.
- After validated tags, automatically build and test bottles, publish GitHub
  Releases with bottle assets and provenance, and automatically publish verified
  formula metadata through protected PRs. These generated metadata-only PRs
  automatically squash-merge after all required checks pass; never auto-approve
  them or bypass branch protections. Verify the immutable ancestor release,
  successful main checks, published bottle digests, and exact generated formula.
  Behavioral formula changes still require normal review and full validation.
  Exempt only generated, stable-source/bottle-metadata-only synchronization commits from
  tagging to prevent a release loop; behavioral formula changes still receive tags.
- Keep release-plz draft planning manual. Automatic GitHub Releases and Homebrew
  bottle hosting are authorized for validated tags from this automation's
  installation onward. Other binary/package publication requires authorization.
  Do not add credentials to release fixtures.
