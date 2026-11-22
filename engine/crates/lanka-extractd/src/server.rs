//! The daemon: runs inside the `tools` container (no network, read-only
//! root, dropped capabilities, memory/pid limits — `infra/v2`), listens on a
//! Unix socket in a volume shared with the worker, and executes one
//! allow-listed [`Op`] per connection under its own [`Limits`].

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Semaphore;

use crate::ops::{self, Limits, Op, Tools};
use crate::proto::{Reply, read_frame, write_frame};

/// Serves until the process is stopped.
pub async fn serve(socket: &Path, tools: Tools, limits: Limits, concurrency: usize) -> Result<()> {
    if socket.exists() {
        std::fs::remove_file(socket).with_context(|| format!("removing stale {}", socket.display()))?;
    }
    let listener = UnixListener::bind(socket).with_context(|| format!("binding {}", socket.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // The worker runs as another uid in the same group.
        std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o660))?;
    }
    tracing::info!(socket = %socket.display(), ?limits, concurrency, "lanka-extractd listening");
    let tools = Arc::new(tools);
    let slots = Arc::new(Semaphore::new(concurrency.max(1)));
    loop {
        let (stream, _) = listener.accept().await?;
        let (tools, slots) = (tools.clone(), slots.clone());
        tokio::spawn(async move {
            let Ok(_permit) = slots.acquire_owned().await else {
                return;
            };
            if let Err(e) = handle(stream, &tools, &limits).await {
                tracing::warn!(error = %format!("{e:#}"), "request failed");
            }
        });
    }
}

async fn handle(mut s: UnixStream, tools: &Tools, limits: &Limits) -> Result<()> {
    let (header, input) = read_frame(&mut s, limits.max_input_mb * 1024 * 1024).await?;
    let result = match serde_json::from_slice::<Op>(&header) {
        Ok(op) => {
            let t0 = std::time::Instant::now();
            let r = ops::run(&op, &input, limits, tools).await;
            tracing::info!(
                op = op.name(),
                input = input.len(),
                ms = t0.elapsed().as_millis() as u64,
                ok = r.is_ok(),
                "op"
            );
            r
        }
        Err(e) => Err(anyhow::anyhow!("bad request: {e}")),
    };
    let (reply, payload) = match result {
        Ok(out) => (Reply { ok: true, error: None }, out),
        Err(e) => (
            Reply {
                ok: false,
                error: Some(format!("{e:#}")),
            },
            Vec::new(),
        ),
    };
    write_frame(&mut s, &serde_json::to_vec(&reply)?, &payload).await
}
