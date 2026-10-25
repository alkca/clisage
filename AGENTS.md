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

Do not merge or release without explicit authorization. For each PR managed,
inspect GitHub Copilot review comments after CI completes, address applicable
recommendations, validate and push follow-up changes. Record why any recommendation
does not apply in the PR workflow summary.
