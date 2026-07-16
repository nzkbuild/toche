use criterion::{Criterion, black_box, criterion_group, criterion_main};
use toche::reduce;
use toche::safe_cache;
use toche::shield;

fn bench_fingerprint_compute(c: &mut Criterion) {
    let body = r#"{"model":"claude-sonnet-5","max_tokens":4096,"messages":[{"role":"user","content":"run cargo test --all"},{"role":"assistant","content":[{"type":"text","text":"Running tests..."}]}],"tools":[{"name":"Bash","description":"Run a shell command","input_schema":{"type":"object","properties":{"command":{"type":"string"}}}}]}"#;
    c.bench_function("fingerprint_compute", |b| {
        b.iter(|| shield::fingerprint::compute(black_box(body)))
    });
}

fn bench_reduce_body(c: &mut Criterion) {
    let body = r#"{
  "model": "claude-sonnet-5",
  "max_tokens": 4096,
  "messages": [
    {"role": "user", "content": "run tests"},
    {"role": "assistant", "content": [
      {"type": "tool_use", "id": "toolu_001", "name": "Bash", "input": {"command": "cargo test --lib"}}
    ]},
    {"role": "user", "content": [
      {"type": "tool_result", "tool_use_id": "toolu_001", "content": "   Compiling my-crate v0.1.0\n   Compiling dep v0.2.0\n    Finished test [unoptimized + debuginfo] target(s) in 2.34s\n     Running unittests src/lib.rs\n\nrunning 50 tests\n..................................................\ntest result: ok. 50 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s"}
    ]},
    {"role": "user", "content": "check git diff"},
    {"role": "assistant", "content": [
      {"type": "tool_use", "id": "toolu_002", "name": "Bash", "input": {"command": "git diff HEAD~5"}}
    ]},
    {"role": "user", "content": [
      {"type": "tool_result", "tool_use_id": "toolu_002", "content": "diff --git a/src/main.rs b/src/main.rs\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,10 +1,15 @@\n+use std::collections::HashMap;\n+\n fn main() {\n     println!(\"hello\");\n+    let map = HashMap::new();\n }\ndiff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -5,3 +5,8 @@\n pub fn add(a: i32, b: i32) -> i32 {\n     a + b\n }\n+\n+pub fn sub(a: i32, b: i32) -> i32 {\n+    a - b\n+}"}
    ]}
  ]
}"#;
    let cfg = reduce::config::ReduceConfig {
        enabled: true,
        command_bypass: vec![],
    };
    c.bench_function("reduce_body_multi_tool", |b| {
        b.iter(|| reduce::transform::reduce_body(black_box(body), black_box(&cfg), black_box(false)))
    });
}

fn bench_safe_cache_workspace_fingerprint(c: &mut Criterion) {
    c.bench_function("workspace_fingerprint", |b| {
        b.iter(|| safe_cache::workspace::compute_workspace_fingerprint())
    });
}

fn bench_inspect_response_safe(c: &mut Criterion) {
    let body = br#"{"type":"message","role":"assistant","content":[{"type":"text","text":"Here is a detailed response explaining the architecture of the system. It covers multiple components including the gateway, the caching layer, the reduction pipeline, and the ledger system. Each component is explained in detail with examples and configuration options."}],"stop_reason":"end_turn","usage":{"input_tokens":1500,"output_tokens":800}}"#;
    c.bench_function("inspect_response_safe", |b| {
        b.iter(|| safe_cache::inspect::inspect_response(black_box(body)))
    });
}

criterion_group!(
    benches,
    bench_fingerprint_compute,
    bench_reduce_body,
    bench_safe_cache_workspace_fingerprint,
    bench_inspect_response_safe,
);
criterion_main!(benches);
