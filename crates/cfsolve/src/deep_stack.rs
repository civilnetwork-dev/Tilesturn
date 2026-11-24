//! Runs recursive work on a thread with a dedicated stack on native targets, or inline on
//! `wasm32` where the linker configures an 8 MiB linear stack.

use anyhow::Result;

/// Stack size for recursive deobfuscation, matching the usual Linux main-thread stack.
#[cfg(not(target_arch = "wasm32"))]
const STACK_SIZE: usize = 8 << 20;

/// Runs `work` on a new thread named `name` and awaits its result without blocking the runtime.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) async fn run<T: Send + 'static>(
    name: &str,
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T> {
    use std::thread;

    use anyhow::Context;
    use tokio::sync::oneshot;

    let (sender, receiver) = oneshot::channel();
    thread::Builder::new()
        .name(name.into())
        .stack_size(STACK_SIZE)
        .spawn(move || {
            // The receiver is gone only if the caller stopped waiting.
            let _ = sender.send(work());
        })
        .with_context(|| format!("starting the {name} thread"))?;
    receiver
        .await
        .with_context(|| format!("the {name} thread failed"))
}

/// Runs `work` inline on `wasm32` (configured with `-zstack-size=8388608`).
#[cfg(target_arch = "wasm32")]
pub(crate) async fn run<T: 'static>(_name: &str, work: impl FnOnce() -> T + 'static) -> Result<T> {
    Ok(work())
}
