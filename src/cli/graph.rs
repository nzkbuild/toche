use crate::config::loader::load_default_integration;
use crate::graphify::adapter::GraphifyAdapter;

fn get_adapter() -> Option<GraphifyAdapter> {
    let resolved = load_default_integration().ok()?;
    let graph_path = resolved
        .graphify
        .as_ref()
        .and_then(|g| g.graph_path.clone())
        .map(std::path::PathBuf::from);
    Some(GraphifyAdapter::new(graph_path))
}

fn check_installed() -> anyhow::Result<()> {
    if !GraphifyAdapter::is_installed() {
        anyhow::bail!(
            "Graphify is not installed. Install with:\n  uv tool install graphifyy\n  pipx install graphifyy"
        );
    }
    Ok(())
}

fn check_graph(adapter: &GraphifyAdapter) -> anyhow::Result<()> {
    let graph_path = adapter.resolve_graph_path();
    if !graph_path.exists() {
        anyhow::bail!(
            "No graph found at {}. Run 'toche graph extract' to build one.",
            graph_path.display()
        );
    }
    Ok(())
}

pub async fn run_query(question: String, budget: Option<u32>) -> anyhow::Result<()> {
    check_installed()?;
    let adapter = get_adapter().unwrap_or_else(|| GraphifyAdapter::new(None));
    check_graph(&adapter)?;

    let result = adapter.query(&question, budget);
    if result.success {
        println!("{}", result.output);
    } else {
        anyhow::bail!("Graphify query failed: {}", result.error);
    }
    Ok(())
}

pub async fn run_path(source: String, target: String) -> anyhow::Result<()> {
    check_installed()?;
    let adapter = get_adapter().unwrap_or_else(|| GraphifyAdapter::new(None));
    check_graph(&adapter)?;

    let result = adapter.path(&source, &target);
    if result.success {
        println!("{}", result.output);
    } else {
        anyhow::bail!("Graphify path failed: {}", result.error);
    }
    Ok(())
}

pub async fn run_explain(node: String) -> anyhow::Result<()> {
    check_installed()?;
    let adapter = get_adapter().unwrap_or_else(|| GraphifyAdapter::new(None));
    check_graph(&adapter)?;

    let result = adapter.explain(&node);
    if result.success {
        println!("{}", result.output);
    } else {
        anyhow::bail!("Graphify explain failed: {}", result.error);
    }
    Ok(())
}

pub async fn run_affected(node: String, depth: Option<u32>) -> anyhow::Result<()> {
    check_installed()?;
    let adapter = get_adapter().unwrap_or_else(|| GraphifyAdapter::new(None));
    check_graph(&adapter)?;

    let result = adapter.affected(&node, depth);
    if result.success {
        println!("{}", result.output);
    } else {
        anyhow::bail!("Graphify affected failed: {}", result.error);
    }
    Ok(())
}

pub async fn run_status() -> anyhow::Result<()> {
    check_installed()?;
    let adapter = get_adapter().unwrap_or_else(|| GraphifyAdapter::new(None));
    let graph_path = adapter.resolve_graph_path();

    println!("Graphify Status");
    println!("===============");
    println!(
        "Binary:     {}",
        which::which("graphify")
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "not found".into())
    );
    println!("Graph file: {}", graph_path.display());
    println!("  exists:   {}", graph_path.exists());

    if graph_path.exists() {
        match adapter.read_graph_stats() {
            Some((nodes, edges, communities)) => {
                println!("  nodes:    {}", nodes);
                println!("  edges:    {}", edges);
                if communities > 0 {
                    println!("  communities: {}", communities);
                }
            }
            None => {
                println!("  (could not parse graph file)");
            }
        }
        if let Ok(meta) = std::fs::metadata(&graph_path) {
            let kb = meta.len() / 1024;
            println!("  size:     {} KB", kb);
        }
    } else {
        println!("  No graph found. Run 'toche graph extract' to build one.");
    }

    Ok(())
}

pub async fn run_extract() -> anyhow::Result<()> {
    check_installed()?;
    let adapter = get_adapter().unwrap_or_else(|| GraphifyAdapter::new(None));

    println!("Building graph with 'graphify extract .' ... (this may take a while)");
    let result = adapter.extract(None);
    if result.success {
        println!("{}", result.output);
    } else {
        anyhow::bail!("Graphify extract failed: {}", result.error);
    }
    Ok(())
}
