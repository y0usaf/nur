mod config;
mod ipc;

use anyhow::Result;
use clap::{Parser, Subcommand};
use gpui::{Application, QuitMode};
use tracing::info;

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

/// nur — GPU-accelerated Lua-scriptable Wayland shell
#[derive(Parser)]
#[command(name = "nur", version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Evaluate a Lua expression in the running nur instance and print the result.
    Eval {
        /// Lua code to evaluate (e.g. `shell.quit()`)
        code: String,
    },
    /// Send a freeform message to the shell.on_msg handler in the running instance.
    Msg {
        /// Message string
        message: String,
    },
    /// Gracefully quit the running nur instance.
    Quit,
    /// Reload the config file in the running nur instance.
    Reload,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env().add_directive("nur=debug".parse()?),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        // ── Client commands ─────────────────────────────────────────────────
        Some(Cmd::Eval { code }) => {
            let resp = ipc::send(ipc::IpcRequest::Eval { code })?;
            ipc::print_response(resp)
        }

        Some(Cmd::Msg { message }) => {
            let resp = ipc::send(ipc::IpcRequest::Msg { message })?;
            ipc::print_response(resp)
        }

        Some(Cmd::Quit) => {
            let resp = ipc::send(ipc::IpcRequest::Quit)?;
            ipc::print_response(resp)
        }

        Some(Cmd::Reload) => {
            let resp = ipc::send(ipc::IpcRequest::Reload)?;
            ipc::print_response(resp)
        }

        // ── Daemon (default — no subcommand) ────────────────────────────────
        None => run_daemon(),
    }
}

fn run_daemon() -> Result<()> {
    let config_path = config::find()?;
    info!("Loading config: {}", config_path.display());

    Application::new()
        .with_assets(assets::source())
        .with_quit_mode(QuitMode::Explicit)
        .run(move |cx| {
            // Start the IPC server before running the Lua config so that
            // shell.on_msg can be registered during config execution.
            if let Err(e) = ipc::start_server(cx, |cx, req| {
                use ipc::{IpcRequest, IpcResponse};
                match req {
                    IpcRequest::Eval { code } => match runtime::eval_lua(cx, code) {
                        Ok(result) => IpcResponse::Ok { result },
                        Err(message) => IpcResponse::Err { message },
                    },
                    IpcRequest::Msg { message } => match runtime::send_msg(cx, message) {
                        Ok(result) => IpcResponse::Ok { result },
                        Err(message) => IpcResponse::Err { message },
                    },
                    IpcRequest::Quit => {
                        cx.quit();
                        IpcResponse::Ok {
                            result: String::new(),
                        }
                    }
                    IpcRequest::Reload => match runtime::eval_lua(cx, "shell.reload()".into()) {
                        Ok(_) => IpcResponse::Ok {
                            result: "reloaded".into(),
                        },
                        Err(message) => IpcResponse::Err { message },
                    },
                }
            }) {
                tracing::warn!("IPC server failed to start: {e}");
            }

            runtime::set_config_path(config_path.clone());

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
