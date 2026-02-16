use anyhow::Result;

mod api;
mod app;
mod config;
mod state;
mod terminal;
mod tools;
mod types;
mod ui;

#[tokio::main]
async fn main() -> Result<()> {
    let config = config::Config::load()?;
    config.validate()?;

    let mut app = app::App::new(config)?;
    app.run().await?;

    Ok(())
}
