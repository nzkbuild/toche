use std::path::PathBuf;
use std::process::Command;

/// Result of a Graphify command execution.
pub struct GraphifyResult {
    pub success: bool,
    pub output: String,
    pub error: String,
}

/// Adapter that wraps the `graphify` CLI binary.
pub struct GraphifyAdapter {
    pub graph_path: Option<PathBuf>,
}

impl GraphifyAdapter {
    pub fn new(graph_path: Option<PathBuf>) -> Self {
        Self { graph_path }
    }

    /// Check if the `graphify` binary is installed on PATH.
    pub fn is_installed() -> bool {
        which::which("graphify").is_ok()
    }

    /// Return the absolute path to graph.json.
    pub fn resolve_graph_path(&self) -> PathBuf {
        self.graph_path
            .clone()
            .unwrap_or_else(|| PathBuf::from("graphify-out").join("graph.json"))
    }

    /// Run a graphify subcommand and return its stdout.
    fn run(&self, args: &[&str]) -> GraphifyResult {
        let child = Command::new("graphify").args(args).output();

        match child {
            Ok(output) => GraphifyResult {
                success: output.status.success(),
                output: String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .to_string(),
                error: String::from_utf8_lossy(&output.stderr)
                    .trim()
                    .to_string(),
            },
            Err(e) => GraphifyResult {
                success: false,
                output: String::new(),
                error: e.to_string(),
            },
        }
    }

    pub fn query(&self, question: &str, budget: Option<u32>) -> GraphifyResult {
        let graph_arg = self.resolve_graph_path().to_string_lossy().to_string();
        let budget_str = budget
            .map(|b| b.to_string())
            .unwrap_or_else(|| "2000".to_string());
        self.run(&[
            "query",
            question,
            "--graph",
            &graph_arg,
            "--budget",
            &budget_str,
        ])
    }

    pub fn path(&self, source: &str, target: &str) -> GraphifyResult {
        let graph_arg = self.resolve_graph_path().to_string_lossy().to_string();
        self.run(&["path", source, target, "--graph", &graph_arg])
    }

    pub fn explain(&self, node: &str) -> GraphifyResult {
        let graph_arg = self.resolve_graph_path().to_string_lossy().to_string();
        self.run(&["explain", node, "--graph", &graph_arg])
    }

    pub fn affected(&self, node: &str, depth: Option<u32>) -> GraphifyResult {
        let graph_arg = self.resolve_graph_path().to_string_lossy().to_string();
        let depth_str;
        let mut args = vec!["affected", node, "--graph", &graph_arg];
        if let Some(d) = depth {
            depth_str = d.to_string();
            args.push("--depth");
            args.push(&depth_str);
        }
        self.run(&args)
    }

    pub fn extract(&self, path: Option<&str>) -> GraphifyResult {
        let target = path.unwrap_or(".");
        self.run(&["extract", target])
    }

    pub fn update(&self, path: Option<&str>) -> GraphifyResult {
        let target = path.unwrap_or(".");
        self.run(&["update", target])
    }

    /// Read graph.json directly to get node/edge counts.
    pub fn read_graph_stats(&self) -> Option<(usize, usize, usize)> {
        let graph_path = self.resolve_graph_path();
        if !graph_path.exists() {
            return None;
        }
        let raw = std::fs::read_to_string(&graph_path).ok()?;
        let parsed: serde_json::Value = serde_json::from_str(&raw).ok()?;
        let nodes = parsed
            .get("nodes")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        let edges = parsed
            .get("edges")
            .or_else(|| parsed.get("links"))
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        let communities = 0;
        Some((nodes, edges, communities))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_default_path_is_none() {
        let adapter = GraphifyAdapter::new(None);
        assert!(adapter.graph_path.is_none());
    }

    #[test]
    fn new_custom_path() {
        let custom = PathBuf::from("/tmp/graph.json");
        let adapter = GraphifyAdapter::new(Some(custom.clone()));
        assert_eq!(adapter.graph_path, Some(custom));
    }

    #[test]
    fn is_installed_does_not_crash() {
        let _ = GraphifyAdapter::is_installed();
    }
}
