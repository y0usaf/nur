//! Unix-domain socket IPC for the `nur` CLI.
//!
//! The running nur daemon listens on `$XDG_RUNTIME_DIR/nur.sock`.
//! CLI subcommands (toggle, eval, quit, msg) connect as a client and
//! send a JSON-encoded `IpcRequest`, then wait for a JSON `IpcResponse`.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IpcRequest {
    /// Evaluate a Lua snippet and return the string representation of the result.
    Eval { code: String },
    /// Send a freeform message string to the `shell.on_msg` handler.
    Msg { message: String },
    /// Gracefully quit the running nur instance.
    Quit,
    /// Reload the config file.
    Reload,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum IpcResponse {
    Ok { result: String },
    Err { message: String },
}

// ---------------------------------------------------------------------------
// Socket path
// ---------------------------------------------------------------------------

pub fn socket_path() -> PathBuf {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(runtime_dir).join("nur.sock")
}

// ---------------------------------------------------------------------------
// Client — sends one request and prints the response
// ---------------------------------------------------------------------------

pub fn send(req: IpcRequest) -> Result<IpcResponse> {
    let path = socket_path();
    let stream = UnixStream::connect(&path)
        .with_context(|| format!("cannot connect to nur socket at {}", path.display()))?;

    send_to_stream(stream, req)
}

fn send_to_stream(mut stream: UnixStream, req: IpcRequest) -> Result<IpcResponse> {
    let msg = serde_json::to_string(&req)? + "\n";
    stream.write_all(msg.as_bytes())?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;

    serde_json::from_str::<IpcResponse>(line.trim()).context("invalid response from nur daemon")
}

// ---------------------------------------------------------------------------
// Server — listens on the socket and dispatches to a callback
// ---------------------------------------------------------------------------

/// Called from the GPUI async task. `handler` is invoked for each incoming
/// request on the main thread via `cx.update`.
pub fn start_server(
    cx: &mut gpui::App,
    handler: impl Fn(&mut gpui::App, IpcRequest) -> IpcResponse + 'static,
) -> Result<()> {
    let path = socket_path();

    // Remove stale socket from a previous run.
    let _ = std::fs::remove_file(&path);

    let listener = UnixListener::bind(&path)
        .with_context(|| format!("cannot bind nur IPC socket at {}", path.display()))?;

    tracing::info!("IPC socket listening at {}", path.display());

    // The handler needs to be callable from inside cx.update, but the
    // async task runs outside of it. We use a channel to send requests
    // from the I/O thread to the GPUI async task, which then calls
    // cx.update to enter the GPUI context.
    let (tx, rx) =
        std::sync::mpsc::channel::<(IpcRequest, std::sync::mpsc::SyncSender<IpcResponse>)>();

    // Spawn a plain OS thread to accept connections (no async runtime needed).
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let tx2 = tx.clone();
                    std::thread::spawn(move || {
                        if let Err(e) = handle_connection(stream, tx2) {
                            tracing::warn!("IPC connection error: {e}");
                        }
                    });
                }
                Err(e) => {
                    tracing::warn!("IPC accept error: {e}");
                    break;
                }
            }
        }
    });

    // GPUI async task: drain the channel and call the handler inside cx.update.
    cx.spawn(async move |cx| {
        loop {
            // Poll the channel every 50ms rather than blocking so we don't
            // stall the GPUI event loop.
            cx.background_executor()
                .timer(std::time::Duration::from_millis(50))
                .await;

            loop {
                let item = match rx.try_recv() {
                    Ok(item) => item,
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
                };

                let (req, resp_tx) = item;
                cx.update(|cx| {
                    let resp = handler(cx, req);
                    let _ = resp_tx.send(resp);
                });
            }
        }
    })
    .detach();

    Ok(())
}

fn handle_connection(
    stream: UnixStream,
    tx: std::sync::mpsc::Sender<(IpcRequest, std::sync::mpsc::SyncSender<IpcResponse>)>,
) -> Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut write_stream = stream;

    let mut line = String::new();
    reader.read_line(&mut line)?;

    let req: IpcRequest = serde_json::from_str(line.trim()).context("invalid request JSON")?;

    let (resp_tx, resp_rx) = std::sync::mpsc::sync_channel(1);
    tx.send((req, resp_tx)).ok();

    let resp = resp_rx.recv().context("daemon did not respond")?;
    let out = serde_json::to_string(&resp)? + "\n";
    write_stream.write_all(out.as_bytes())?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers to format IPC results for the terminal
// ---------------------------------------------------------------------------

pub fn print_response(resp: IpcResponse) -> Result<()> {
    match resp {
        IpcResponse::Ok { result } => {
            if !result.is_empty() {
                println!("{result}");
            }
            Ok(())
        }
        IpcResponse::Err { message } => {
            bail!("{message}");
        }
    }
}
