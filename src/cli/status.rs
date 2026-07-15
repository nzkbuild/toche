use crate::profiles::loader::load_profiles;

pub async fn run() -> anyhow::Result<()> {
    println!("Toche Status");
    println!("============");

    match load_profiles() {
        Ok(profiles) => {
            println!(
                "Default profile: {}",
                profiles.default.as_deref().unwrap_or("none")
            );
            for p in &profiles.profiles {
                println!("  {} -> {}", p.name, p.upstream_url);
            }
        }
        Err(e) => {
            println!("No profiles loaded: {e}");
        }
    }

    Ok(())
}
