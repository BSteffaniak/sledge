//! Conservative Linux lifecycle: no device access or authorization bypasses.
use super::{MatcherSink, MatchingState, ReloadFn, reload_callback};
use crate::cli::Cli;
use crate::ipc::{self, ServerState};
use crate::logging;
use anyhow::{Context, Result};
use parking_lot::Mutex;
use sledge_core::{EventSink, InputBackend};
use sledge_linux::{LinuxBackend, ShutdownHandle};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{info, warn};

pub(super) fn run_linux(
    cli: &Cli,
    args: &crate::cli::RunArgs,
    matcher: Arc<Mutex<MatchingState>>,
    rules_loaded: Arc<Mutex<usize>>,
    config_path: Arc<PathBuf>,
    reload_handle: logging::FilterReloadHandle,
) -> Result<()> {
    let desktop = super::desktop::start(args)?;
    let desktop_focus = desktop
        .as_ref()
        .map(sledge_linux::desktop::worker::DesktopWorker::focus);
    let mut backend = LinuxBackend::new();
    if let Some(path) = &args.linux_broker_socket {
        backend
            .connect_broker(path)
            .context("connecting privileged Linux broker")?;
    }
    let focus = backend.focus_tracker();
    let shutdown = backend.shutdown_handle();
    let snapshot = backend.clone();
    let inject_backend = Arc::new(backend.clone());
    let source_work = if let Some(desktop) = &desktop {
        let dispatcher = desktop.dispatcher();
        Some(super::source_queue::SourceWorker::start(
            desktop.sources(),
            move |catalog, id| {
                dispatcher.select(catalog, id).map_err(|error| match error {
                    sledge_linux::desktop::DesktopError::UnknownSource => {
                        sledge_core::BackendError::UnknownInputSource(id.to_owned())
                    }
                    other => sledge_core::BackendError::Inject(other.to_string()),
                })
            },
        )?)
    } else {
        None
    };
    let sources = source_work.as_ref().map(|(queue, _)| queue.clone());
    let sink: Box<dyn EventSink> = Box::new(MatcherSink {
        matcher: matcher.clone(),
        backend: inject_backend,
        sources,
        focus: desktop_focus.clone(),
    });
    let last_reload_at = Arc::new(Mutex::new(None));
    let reload = reload_callback(
        matcher,
        rules_loaded.clone(),
        config_path.clone(),
        last_reload_at.clone(),
        reload_handle,
    );
    let sources_configured = source_work.is_some();
    let state = Arc::new(ServerState {
        started_at: Instant::now(),
        rules_loaded,
        focused_app: Arc::new(move || match &desktop_focus {
            Some(cache) => cache.current().map(|identity| identity.value),
            None => focus.current(),
        }),
        reload: reload.clone(),
        check_permissions: Arc::new(|| None),
        capabilities: Arc::new(move || {
            let caps = snapshot.capabilities();
            Some(serde_json::json!({
                "broker_connected": caps.broker_connected,
                "verified_session": caps.verified_session,
                "source_switching": sources_configured,
                "native_send_key": false,
                "strict_lock_authority": false,
                "reason": caps.reason,
            }))
        }),
        last_reload_at,
    });
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(2)
        .build()?;
    let path = crate::status::socket_path(cli)?;
    let (listener, _socket_guard) = sledge_linux::ipc::SocketGuard::bind(&path).context(
        "binding private same-user IPC socket (existing paths require explicit recovery)",
    )?;
    listener.set_nonblocking(true)?;
    let listener = {
        let _entered = runtime.enter();
        tokio::net::UnixListener::from_std(listener)?
    };
    let server = runtime.spawn(ipc::serve(listener, state));
    let _signals = SignalThread::spawn(reload.clone(), shutdown)?;
    let _watcher = crate::config_watcher::spawn((*config_path).clone(), reload);
    // The backend's fail-closed authorization remains authoritative. This CLI
    // does not open devices, connect a privileged broker, or invent permissions.
    let result = backend.run(sink).map_err(anyhow::Error::from);
    server.abort();
    runtime.shutdown_timeout(Duration::from_secs(5));
    drop(source_work);
    drop(desktop);
    result
}

struct SignalThread {
    handle: signal_hook::iterator::Handle,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl SignalThread {
    fn spawn(reload: ReloadFn, shutdown: ShutdownHandle) -> Result<Self> {
        use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
        let mut signals = signal_hook::iterator::Signals::new([SIGHUP, SIGINT, SIGTERM])?;
        let handle = signals.handle();
        let thread = std::thread::Builder::new()
            .name("sledge-signals".into())
            .spawn(move || {
                for signal in signals.forever() {
                    if signal == SIGHUP {
                        match reload() {
                            Ok(()) => info!("reload via SIGHUP"),
                            Err(error) => warn!(%error, "SIGHUP reload failed"),
                        }
                    } else {
                        info!(signal, "shutdown requested");
                        shutdown.shutdown();
                        break;
                    }
                }
            })?;
        Ok(Self {
            handle,
            thread: Some(thread),
        })
    }
}
impl Drop for SignalThread {
    fn drop(&mut self) {
        self.handle.close();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
