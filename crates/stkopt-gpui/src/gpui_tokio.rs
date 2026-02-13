//! Bridge between GPUI and Tokio async runtimes.
//!
//! This module provides utilities to spawn Tokio futures and get results back
//! as GPUI tasks, enabling seamless integration between the two async runtimes.
//!
//! Based on https://github.com/zed-industries/zed/tree/main/crates/gpui_tokio

use std::future::Future;

use gpui::{App, AppContext, Global, ReadGlobal, Task};

pub use tokio::task::JoinError;

/// Initializes the Tokio wrapper using a new Tokio runtime with 2 worker threads.
pub fn init(cx: &mut App) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("Failed to initialize Tokio runtime");

    cx.set_global(GlobalTokio::new(RuntimeHolder::Owned(runtime)));
}

/// Initializes the Tokio wrapper using an existing Tokio runtime handle.
pub fn init_from_handle(cx: &mut App, handle: tokio::runtime::Handle) {
    cx.set_global(GlobalTokio::new(RuntimeHolder::Shared(handle)));
}

enum RuntimeHolder {
    Owned(tokio::runtime::Runtime),
    Shared(tokio::runtime::Handle),
}

impl RuntimeHolder {
    pub fn handle(&self) -> &tokio::runtime::Handle {
        match self {
            RuntimeHolder::Owned(runtime) => runtime.handle(),
            RuntimeHolder::Shared(handle) => handle,
        }
    }
}

struct GlobalTokio {
    runtime: RuntimeHolder,
}

impl Global for GlobalTokio {}

impl GlobalTokio {
    fn new(runtime: RuntimeHolder) -> Self {
        Self { runtime }
    }
}

/// Helper for deferring cleanup on drop.
struct Defer<F: FnOnce()>(Option<F>);

impl<F: FnOnce()> Drop for Defer<F> {
    fn drop(&mut self) {
        if let Some(f) = self.0.take() {
            f();
        }
    }
}

fn defer<F: FnOnce()>(f: F) -> Defer<F> {
    Defer(Some(f))
}

/// Tokio integration for GPUI.
pub struct Tokio;

impl Tokio {
    /// Spawns the given future on Tokio's thread pool, and returns it via a GPUI task.
    /// Note that the Tokio task will be cancelled if the GPUI task is dropped.
    pub fn spawn<C, Fut, R>(cx: &C, f: Fut) -> Task<Result<R, JoinError>>
    where
        C: AppContext<Result<Task<Result<R, JoinError>>> = Task<Result<R, JoinError>>>,
        Fut: Future<Output = R> + Send + 'static,
        R: Send + 'static,
    {
        cx.read_global(|tokio: &GlobalTokio, cx| {
            let join_handle = tokio.runtime.handle().spawn(f);
            let abort_handle = join_handle.abort_handle();
            let cancel = defer(move || {
                abort_handle.abort();
            });
            cx.background_spawn(async move {
                let result = join_handle.await;
                drop(cancel);
                result
            })
        })
    }

    /// Spawns the given future on Tokio's thread pool, and returns it via a GPUI task.
    /// This version unwraps the JoinError into an anyhow::Result.
    pub fn spawn_result<C, Fut, R>(cx: &C, f: Fut) -> Task<anyhow::Result<R>>
    where
        C: AppContext<Result<Task<anyhow::Result<R>>> = Task<anyhow::Result<R>>>,
        Fut: Future<Output = anyhow::Result<R>> + Send + 'static,
        R: Send + 'static,
    {
        cx.read_global(|tokio: &GlobalTokio, cx| {
            let join_handle = tokio.runtime.handle().spawn(f);
            let abort_handle = join_handle.abort_handle();
            let cancel = defer(move || {
                abort_handle.abort();
            });
            cx.background_spawn(async move {
                let result = join_handle.await?;
                drop(cancel);
                result
            })
        })
    }

    /// Gets the Tokio runtime handle.
    pub fn handle(cx: &App) -> tokio::runtime::Handle {
        GlobalTokio::global(cx).runtime.handle().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_defer_runs_on_drop() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};

        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        {
            let _d = defer(move || {
                called_clone.store(true, Ordering::SeqCst);
            });
        }

        assert!(called.load(Ordering::SeqCst));
    }
}
