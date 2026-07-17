use crate::config::loader::load_config;

pub async fn run() -> anyhow::Result<()> {
    println!("Toche Status");
    println!("============");

    match load_config() {
        Ok(config) => {
            let default_name = config
                .defaults
                .integration
                .and_then(|id| config.integrations.iter().find(|i| i.id == id))
                .map(|i| i.name.clone())
                .unwrap_or_else(|| "none".into());
            println!("Default integration: {default_name}");
            for i in &config.integrations {
                let upstream = config
                    .upstreams
                    .iter()
                    .find(|u| u.id == i.upstream)
                    .map(|u| u.url.as_str())
                    .unwrap_or("unknown");
                println!("  {} -> {}", i.name, upstream);
            }
        }
        Err(e) => {
            println!("No configuration loaded: {e}");
        }
    }

    Ok(())
}
