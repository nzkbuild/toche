# Toche 1.1.0 — Release Publication Assignment

**To:** Sonnet (Release Engineer)  
**From:** Astra (Release Director)  
**Commit:** `805eef0` (v1.1.0)  
**Branch:** `feat/1.1.0-multi-client-runtime`

---

## Mission

Toche 1.1.0 has passed implementation verification (M17). Publish it.

---

## Pre-Flight Checks

Before publishing, verify:

1. Working tree is clean: `git status` shows nothing
2. Version in `Cargo.toml` is exactly `1.1.0`
3. All commits from M08–M17 are present on the branch
4. `cargo clippy --all-targets --all-features -- -D warnings` passes
5. `cargo test --all-features` passes (377/378 — the 1 flaky test is documented and accepted)
6. `cargo fmt --all -- --check` passes
7. `cargo audit` returns zero vulnerabilities
8. `cargo deny check` passes all categories

---

## Publication Steps

Execute in order:

1. **Push all commits** to `origin/feat/1.1.0-multi-client-runtime`
2. **Merge to main** (or rebase, per project convention)
3. **Create annotated tag:** `git tag -a v1.1.0 -m "Toche v1.1.0 — Multi-Client Runtime"`
4. **Push tag:** `git push origin v1.1.0`
5. **Create GitHub Release:**
   - Tag: `v1.1.0`
   - Title: `Toche v1.1.0 — Multi-Client Runtime`
   - Body: from `docs/releases/v1.1.0.md`
   - Attach any binary artifacts if applicable
6. **Publish to crates.io:** `cargo publish` (verify `Cargo.toml` has correct metadata first)
7. **Verify:** `cargo install toche` from a fresh environment, check `toche --version` returns `1.1.0`

---

## Confirmation

Report back:

- [ ] All commits pushed
- [ ] tag `v1.1.0` pushed
- [ ] GitHub Release created with URL
- [ ] crates.io published with URL
- [ ] Fresh install verified (`toche --version` → `1.1.0`)
- [ ] M17 verification report committed and pushed

---

Begin.
