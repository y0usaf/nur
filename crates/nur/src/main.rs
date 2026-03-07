mod config;

use anyhow::Result;
use gpui::Application;
use tracing::info;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("nur=debug".parse()?),
        )
        .init();

    let config_path = config::find()?;
    info!("Loading config: {}", config_path.display());

    Application::new().with_assets(assets::source()).run(move |cx| {
        let runtime = runtime::LuaRuntime::new();
        if let Err(e) = runtime.run(&config_path, cx) {
            tracing::error!("{e:#}");
            cx.quit();
        }

        // Keep the runtime alive for the duration of the process so that
        // render callbacks and timer closures can still reach the Lua VM.
        cx.set_global(runtime);
    });

    Ok(())
}
