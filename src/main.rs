use color_eyre::eyre::Result;

fn main() -> Result<()> {
    color_eyre::install()?;
    let args: Vec<String> = std::env::args().collect();
    if let Some(cmd) = args.get(1).map(|s| s.as_str()) {
        match cmd {
            "config" => {
                ripl::config::open_config_file()?;
                return Ok(());
            }
            "pair" => {
                let provider = args.get(2).map(|s| s.as_str()).unwrap_or("");
                ripl::config::pair_provider(provider)?;
                return Ok(());
            }
            _ => {}
        }
    }
    ripl::run()
}
