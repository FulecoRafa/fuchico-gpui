//! A small shared tokio runtime so a GUI thread (which must never block) can
//! spawn CalDAV work.

use std::future::Future;
use std::sync::OnceLock;

use tokio::runtime::{Handle, Runtime};

fn runtime() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("fuchico-core")
            .enable_all()
            .build()
            .expect("failed to build tokio runtime")
    })
}

/// Handle to the shared runtime.
pub fn handle() -> Handle {
    runtime().handle().clone()
}

/// Spawns a future onto the shared runtime (fire and forget / join handle).
pub fn spawn<F>(fut: F) -> tokio::task::JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    runtime().spawn(fut)
}

/// Runs `fut` to completion on the shared runtime and delivers the result to
/// `done` (on a runtime thread). Never blocks the caller.
pub fn spawn_with<F, T>(fut: F, done: impl FnOnce(T) + Send + 'static)
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    runtime().spawn(async move { done(fut.await) });
}

/// Blocks the current (non-runtime) thread on `fut`. For tests/CLIs only.
pub fn block_on<F: Future>(fut: F) -> F::Output {
    runtime().block_on(fut)
}
