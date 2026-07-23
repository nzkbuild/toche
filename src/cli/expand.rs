use anyhow::Context;

use crate::config::loader::load_config;
use crate::profiles::loader::config_dir;
use crate::reduce;

pub async fn run(hash: String, json: bool) -> anyhow::Result<()> {
    let config = load_config().context("Failed to load configuration")?;
    let (_, cas_dir) = config.storage.resolve_paths(&config_dir());
    match reduce::storage::retrieve_at(&hash, &cas_dir) {
        Ok(bytes) => {
            if json {
                let content = String::from_utf8_lossy(&bytes).to_string();
                let output = serde_json::json!({
                    "found": true,
                    "hash": hash,
                    "content": content,
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&output)
                        .context("Failed to serialize expand output")?
                );
            } else {
                std::io::Write::write_all(&mut std::io::stdout(), &bytes)
                    .context("Failed to write to stdout")?;
            }
        }
        Err(_) => {
            if json {
                let output = serde_json::json!({
                    "found": false,
                    "hash": hash,
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&output)
                        .context("Failed to serialize expand output")?
                );
            } else {
                eprintln!("Error: CAS blob not found for hash: {hash}");
                std::process::exit(1);
            }
        }
    }
    Ok(())
}
