# Toche 1.1.0 — Implementation Verification Assignment

**To:** Sonnet (Author and Implementation Engineer)  
**From:** Astra (Engineering Director)  
**Commit:** `805eef0` (v1.1.0, branch `feat/1.1.0-multi-client-runtime`)  
**Classification:** Pre-Release Verification — Production Gate

---

## Mission

Toche 1.1.0 is version-bumped and milestone-complete. Your task is to **attempt to disprove** that it is production-ready.

- Never assume correctness. Verify.
- No speculation. Evidence only.
- If something passes, explain *why* it is safe — don't just assert.
- Think adversarially: malicious user, corrupted state, interrupted execution, wrong platform.

---

## Required Verification Areas

You must cover every area below. Expand beyond this list whenever appropriate.

### 1. Functionality & Correctness
- Multi-client routing: Anthropic (`/v1/messages`) vs OpenAI Responses (`/v1/responses`) — do both protocols deliver correct responses end-to-end?
- Flight key derivation: `{url}|{fingerprint}|{trust_domain_id}|{policy_hash}` — is key collision possible? Are all four components stable across restarts?
- Coalescing: do concurrent Claude Code flights to the same upstream coalesce? Do concurrent Codex flights NOT coalesce? Does cross-protocol coalescing correctly NOT happen?
- Trust domain isolation: do two Claude Code instances with different trust domains produce different `TrustDomainId` values? Is one prevented from reading the other's cache?
- Identity persistence: does `RuntimeId` survive restart? Does `TrustDomainId` survive restart?
- Schema migration: v10 → v11 ledger migration — is it truly idempotent? Run it 5 times. Same result each time?

### 2. CLI Behaviour
- `toche stats` — correct output format? JSON schema version matches manifest?
- `toche setup` — idempotent? What happens if run mid-flight?
- `/status` HTTP endpoint — live flights count accurate? Protocol counts correct?
- `toche --version` — returns `1.1.0`?
- `toche --help` — all subcommands documented? No dead references?

### 3. Edge Cases & Invalid Inputs
- Unknown model name → correct 400 response with descriptive error?
- Malformed config.toml → descriptive error, not panic?
- Non-Git workspace → graceful rejection?
- Dirty Git workspace — what happens?
- Config with missing required fields → what error?
- Config with duplicate integration names → what happens?
- Config referencing nonexistent workspace paths → what happens?
- Extremely long model names, API keys, workspace paths?
- Unicode in config values?

### 4. Malformed Data & Corrupted State
- Corrupt `ledger.db` — partial write, truncated file, all zeros, random bytes. What happens?
- Corrupt `config.toml` — truncated, binary garbage, valid TOML with wrong schema. What happens?
- Corrupt CAS files — wrong hash, truncated, missing. What happens?
- CAS directory with unexpected files or permissions — what happens?

### 5. Recovery & Interrupted Execution
- Kill `toche` mid-flight (SIGTERM, SIGKILL on Linux/macOS; Task Manager kill on Windows). Then:
  - Can it restart cleanly?
  - Does the ledger have orphaned flight entries?
  - Does the CAS have partial files?
  - Does the config hash remain valid?
- Kill during `toche setup` — is partial setup state detected on next run?
- Kill during schema migration — is the ledger left in a recoverable state?
- Power-loss simulation: write a ledger entry, kill before fsync. What is the ledger state on restart?

### 6. Migrations & Backwards Compatibility
- Create a v1.0.10 config (`profiles.toml` format). Migrate to v1.1.0. Migrate again. Lossless both ways?
- Create a v1.0.10 ledger (schema v10). Open with v1.1.0. Does it upgrade? Is the old data intact?
- What happens if a v1.1.0 ledger is opened by v1.0.10 code? Should it reject cleanly?
- Config roundtrip: v1.0.10 → load → save → reload. Are all values preserved? Any normalization?

### 7. Cache Integrity (CAS)
- Store content, kill process, retrieve — correct?
- Store content twice — idempotent? Correct hash both times?
- What if CAS directory is read-only?
- What if CAS directory is full?
- Concurrent writes to same hash?

### 8. Platform Compatibility
You must test on all three platforms with actual execution, not code review:

#### Windows
- File locking: does the ledger handle Windows mandatory locks?
- Path separators: any hardcoded `/` that should be `\`? Any `C:\` assumptions?
- Long paths (>260 chars)?
- Line endings: any CRLF corruption in config or ledger?
- Process kill via Task Manager: recovery behaviour?

#### Linux
- All functionality on a clean install?
- Ledger fsync behaviour?
- Signal handling (SIGTERM vs SIGKILL)?
- /tmp usage for any temp files?

#### macOS
- All functionality?
- Any macOS-specific path issues (case-insensitive filesystem)?
- SIP-related restrictions?

### 9. Concurrency & Race Conditions
- 10 simultaneous flights to same upstream — coalescing still correct? Token counts right?
- 10 simultaneous flights to different upstreams — no cross-contamination?
- Simultaneous `toche setup` and `toche stats` — any panic?
- Simultaneous flight and ledger query — any deadlock?
- Simultaneous CAS write and read — any corruption?
- `MutexGuard` held across `.await` — verify zero occurrences across all test and src files with clippy.

### 10. Panic Paths & Memory Safety
- Run with `RUST_BACKTRACE=full`. Any unexpected panics?
- Search all `unwrap()` and `expect()` calls in `src/` (not tests). For each:
  - Is the panic guaranteed impossible? Prove it.
  - Could an attacker trigger it with crafted input? If yes, FAIL.
- Search for `unsafe` blocks. Any? If yes, audit each.
- Search for `.lock().unwrap()` — any risk of poisoned mutex? (PoisonError after a panic in another thread)

### 11. Serialization & Deserialization
- Config serialization: roundtrip `Config` → TOML → `Config`. Lossless?
- Ledger serialization: write flight → read flight. All fields preserved?
- JSON responses from `/status` — valid JSON? Schema matches?
- What happens with unexpected JSON fields in API responses from upstream?
- What happens with missing JSON fields in API responses from upstream?

### 12. Configuration Parsing
- Valid `profiles.toml` (v1.0.10 format) — loads correctly?
- Valid `config.toml` (v1.1.0 format) — loads correctly?
- Mixed schema (some v1.0.10, some v1.1.0) — what happens?
- Environment variable overrides — do they work? Priority correct?
- Config with BOM (byte order mark) at start?
- Config with trailing whitespace?

### 13. Performance
- Measure `toche stats` time with 1000 flights in ledger. Acceptable?
- Measure gateway startup time. Acceptable?
- Measure end-to-end request latency (through Toche vs direct to upstream). Overhead?
- Check for unnecessary allocations: any `clone()` that could be a reference? Any `collect()` that could be an iterator?
- Disk IO: does every flight write synchronously? Should any be batched?

### 14. Dependency Audit
- `cargo audit` — zero vulnerabilities?
- `cargo deny check` — advisories, bans, licenses, sources all pass?
- Any dependency pulling in 50+ transitive crates unnecessarily?
- Any yanked crates in the lockfile?

### 15. Code Quality Gates
- `cargo clippy --all-targets --all-features -- -D warnings` — zero warnings?
- `cargo fmt --all -- --check` — clean?
- `cargo doc` — zero warnings?

### 16. Documentation Accuracy
- README.md: every command example actually works when copy-pasted?
- CHANGELOG.md: all entries match actual commits? No missing changes?
- CONTRIBUTING.md: setup instructions produce a working dev environment?
- ARCHITECTURE.md: crate map matches actual `src/` directory structure? ADRs still accurate?
- Code comments: any doc comment that claims something untrue? (`cargo doc` and spot-check)

### 17. Installation & First-Run
- Fresh `cargo install --path .` — works?
- First run (`toche setup`) on a machine with no prior Toche — completes?
- First run in a non-Git directory — graceful error?
- Upgrade from v1.0.10 — old config found and migrated?

### 18. Regression Testing
- Run the full test suite 10 times. Any flaky failures? Record every flake.
- Known flaky tests: `same_creds_different_workspace_isolation`, `power_loss_simulation`, `killed_runtime_recovery`. Are these still flaky?
- For each flaky test: what is the root cause? Is it a test issue or a production bug?

### 19. Missing Tests
- Run `cargo tarpaulin` or `cargo llvm-cov`. What branches are uncovered?
- For each uncovered branch: is it unreachable by design, or is coverage missing?
- Any public function with zero test coverage?
- Any error path with zero test coverage?
- Any recovery path with zero test coverage?

### 20. Attack Surface
- Try to inject shell commands through API key field in config.
- Try path traversal in workspace path config (e.g., `../../etc/passwd`).
- Try to overflow string fields (100KB model name, 1MB API key).
- Try to send a request with a malicious `X-Forwarded-For` header — does Toche forward it?
- Try to send a request with `\r\n` in headers — HTTP injection?

---

## Testing Philosophy

Do not simply execute existing tests. They already pass.

- Think like a **malicious user** attempting to crash or exploit Toche.
- Think like an **enterprise customer** running Toche on 100 machines simultaneously.
- Think like **CI** in a loop running `toche setup` and `toche stats` 1000 times.
- Think like **Windows** — different process model, different filesystem, different path separators.
- Think like a **developer using Toche incorrectly** — wrong config, wrong directory, wrong version.
- Think like someone **attempting to corrupt the cache** — write garbage to CAS, delete ledger mid-flight.
- Think like someone who **accidentally interrupts execution halfway through** — Ctrl+C during setup, kill during migration.

**Attempt to break every assumption in the codebase.**

---

## Evidence Standard

Every finding must include:

```markdown
### Finding #[N]: [Title]

- **Severity:** Critical / High / Medium / Low / Info
- **Affected files:** [paths]
- **Affected functions:** [names]
- **Evidence:** [exact command output, stack trace, screenshot]
- **Reproduction:**
  1. [Step 1]
  2. [Step 2]
  3. [Step 3]
- **Expected behaviour:** [what should happen]
- **Actual behaviour:** [what actually happened]
- **Root cause:** [why]
- **Production impact:** [what it means for users]
- **Recommended fix:** [specific change]
```

**No speculation. No guesses. Evidence only.**

If no issue exists in a verification area, explain **why it is safe** with concrete reasoning, not "looks good."

---

## Deliverables

Produce a single markdown report saved to `docs/roadmaps/v1.1.0/evidence/M17-sonnet-verification.md`.

The report must include:

1. **Executive Summary** (1 paragraph — can a busy director skip the rest?)
2. **Overall Release Health** (semaphore: 🟢 go / 🟡 caution / 🔴 stop)
3. **Functional Verification** (protocol correctness, routing, coalescing, identity)
4. **Stress Testing Results** (concurrency, race conditions, 1000-flight scenarios)
5. **Edge Case Results** (invalid inputs, malformed data, corrupted state)
6. **Platform Compatibility** (Windows, Linux, macOS — each with actual execution evidence)
7. **Performance Findings** (latency, allocations, disk IO)
8. **Security Findings** (cargo audit, cargo deny, injection attempts, path traversal)
9. **Documentation Verification** (command accuracy, architecture accuracy)
10. **Test Coverage Assessment** (coverage %, uncovered branches, missing tests)
11. **Remaining Risks** (what could still go wrong in production)
12. **Technical Observations** (anything interesting not covered above)
13. **Production Readiness Verdict** (Ready / Ready with Caveats / Not Ready)
14. **Deployment Readiness Verdict** (Ready / Ready with Caveats / Not Ready)
15. **Final Go / No-Go Recommendation**

---

## Constraints

- Do not review architecture, roadmap, or design philosophy — that is Astra's responsibility.
- Do not propose new features or scope changes.
- Focus strictly on: does the current code at `805eef0` work correctly in production?
- Commit your report file. Do not modify source code unless fixing a verified bug.

---

Begin.
