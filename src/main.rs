use aistar::{app::App, config::Config};
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::load()?;
    config.validate()?;

    let mut app = App::new(config)?;
    app.run().await?;

    Ok(())
}
