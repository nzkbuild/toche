# M12 Failure Hardening — Failure Mode Matrix

All 29 failure modes tested across shards 1–5, with test name, shard, and status.

| # | Mode | Test Name | Shard | Status |
|---|---|---|---|---|
| 1 | runtime-before-setup | `runtime_before_setup_status_returns_active_zero` | 1 | PASS |
| 2 | setup-idempotent | `setup_idempotent_connect_twice_no_duplicates` | 1 | PASS |
| 3 | endpoint-offline | `endpoint_offline_returns_502` | 1 | PASS |
| 4 | endpoint-fails-mid-stream | `endpoint_fails_mid_stream_partial_response_not_cached` | 1 | PASS |
| 5 | unknown-model | `unknown_model_returns_400_with_clear_error` | 1 | PASS |
| 6 | no-usage-metadata | `no_usage_metadata_reports_usage_as_unknown` | 1 | PASS |
| 7 | multi-claude-diff-trust-domains | `multi_claude_two_instances_diff_trust_domains_no_credential_crossover` | 2 | PASS |
| 8 | multi-codex-diff-trust-domains | `multi_codex_two_instances_diff_trust_domains` | 2 | PASS |
| 9 | claude+codex-simultaneous | `claude_plus_codex_simultaneous_diff_protocols` | 2 | PASS |
| 10 | different-creds-same-url | `different_creds_same_url_routing_correct` | 2 | PASS |
| 11 | same-creds-diff-workspace | `same_creds_different_workspace_isolation` | 2 | PASS |
| 12 | non-git-workspace | `non_git_workspace_no_crash` | 3 | PASS |
| 13 | dirty-git-workspace | `dirty_git_workspace_succeeds` | 3 | PASS |
| 14 | ledger-locked | `ledger_locked_traffic_passes` | 3 | PASS |
| 15 | malformed-config | `malformed_config_descriptive_error` | 3 | PASS |
| 16 | interrupted-setup | `interrupted_setup_no_partial_fragment` | 3 | PASS |
| 17 | killed-runtime-recovery | `killed_runtime_recovery` | 4 | PASS |
| 18 | power-loss-simulation | `power_loss_simulation` | 4 | PASS |
| 19 | downgrade-attempt-rejected | `downgrade_attempt_rejected` | 4 | PASS |
| 20 | newer-schema-detected | `newer_schema_detected` | 4 | PASS |
| 21 | self-signed-tls-no-unsafe-bypass | `self_signed_tls_no_unsafe_bypass` | 4 | PASS |
| 22 | upstream-changed-after-setup | `upstream_changed_after_setup` | 4 | PASS |
| 23 | unknown-headers-forwarded | `unknown_headers_forwarded` | 5 | PASS |
| 24 | upstream-rejects-toche-headers | `upstream_rejects_toche_headers` | 5 | PASS |
| 25 | binary-content-passthrough | `binary_content_passthrough` | 5 | PASS |
| 26 | already-reduced-content | `already_reduced_content` | 5 | PASS |
| 27 | multi-claude-concurrent-flights | `multi_claude_concurrent_flights` | 5 | PASS |
| 28 | multi-codex-concurrent-flights | `multi_codex_concurrent_flights` | 5 | PASS |
| 29 | claude-codex-no-cross-protocol-coalesce | `claude_codex_concurrent_no_cross_protocol_coalesce` | 5 | PASS |

## Coverage Map

| Category | Modes | Shards |
|---|---|---|
| Startup / Bootstrap | 1, 2, 5, 12, 13, 15, 16, 17, 18, 19, 20 | 1, 3, 4 |
| Network / Transport | 3, 4, 21, 23, 25 | 1, 4, 5 |
| Multi-Client / Multi-Protocol | 7, 8, 9, 10, 11, 27, 28, 29 | 2, 5 |
| Reporting / Ledger | 6, 14 | 1, 3 |
| Resilience / Recovery | 17, 18, 22 | 4 |
| Content / Reduce | 24, 25, 26 | 5 |
