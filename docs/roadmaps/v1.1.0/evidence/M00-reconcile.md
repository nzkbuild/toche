# M00 — Reconcile the real source of truth

## Commit SHA

`3a2a3e13dff897edcc51ce690e4ccd5f7af0c049`

## Files changed

- Created `docs/roadmaps/v1.1.0/evidence/M00-reconcile.md` (this file).
- Preserved untracked `docs/roadmaps/1.1.0_ROADMAP.md` (remains in working tree, not staged).

## Reused sources involved

None for M00. This milestone only reconciles the local workspace to the public `v1.0.10` source.

## Commands run

```shell
git status --short
git remote -v
git branch --show-current
git rev-parse HEAD
git tag --points-at HEAD
git fetch origin --tags --prune
git rev-parse "v1.0.10^{commit}"
git log --oneline --decorate v1.0.6..v1.0.10
git switch -c feat/1.1.0-multi-client-runtime v1.0.10
cargo metadata --no-deps --format-version 1
node -p "require('./package.json').version"
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features --locked
npm run test:npm
cargo bench
```

## Tests passed

- `cargo test --all-features --locked`: **154 unit tests passed**, 131 lib tests passed, integration tests passed.
- `npm run test:npm`: **5/5 passed**.

## Benchmarks changed

- `cargo bench` executed successfully.
- Benchmarks available: `fingerprint_compute`, `reduce_body_multi_tool`, `workspace_fingerprint`, `inspect_response_safe`.
- No baseline comparison required for M00; baseline captured for future M15 comparison.

## Known limitations

- Working tree contains one untracked file (`docs/roadmaps/1.1.0_ROADMAP.md`) which was preserved, not overwritten.
- No destructive Git commands were used.
- No tags were moved or rewritten.

## Acceptance-gate result

PASS. Branch ancestry begins at exact public `v1.0.10` (`3a2a3e13dff897edcc51ce690e4ccd5f7af0c049`), versions report `1.0.10`, all Rust tests pass, npm installer tests pass, formatting passes, Clippy passes with warnings denied, and existing benchmarks execute.

## Next unlocked milestone

M01 — Freeze the contract and current-reality audit.
