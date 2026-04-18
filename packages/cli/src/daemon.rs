//! Daemon main loop: wire backend + matcher + IPC, handle signals.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use parking_lot::Mutex;
use sledge_config::models::AppAlias;
use sledge_core::{BackendVerdict, EventSink, InputBackend, KeyEvent, Matcher, Verdict};
use tracing::{info, warn};

use crate::cli::{Cli, RunArgs};
use crate::ipc::{self, ServerState, StatusPermissions};
use crate::logging;

/// Run the daemon.
///
/// # Errors
///
/// Returns an error if the backend cannot be installed or the config
/// cannot be loaded.
pub fn run(cli: &Cli, args: RunArgs) -> Result<()> {
    let config_path = resolve_config_path(cli)?;
    let initial = sledge_config::load_from_file(&config_path)
        .with_context(|| format!("loading {}", config_path.display()))?;

    let _guard =
        logging::init(&initial.daemon.log_level, args.stdout_logs).context("tracing init")?;

    info!(
        version = env!("CARGO_PKG_VERSION"),
        config = %config_path.display(),
        "sledge starting"
    );

    let matcher = Arc::new(Mutex::new(Matcher::new(initial.rules.clone())));
    let rules_loaded = Arc::new(Mutex::new(initial.rules.len()));
    let aliases = Arc::new(initial.app_aliases.clone());
    let config_path = Arc::new(config_path);

    #[cfg(target_os = "macos")]
    {
        run_macos(cli, matcher, rules_loaded, config_path, aliases)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (matcher, rules_loaded, config_path, aliases);
        anyhow::bail!("only the macOS backend is implemented")
    }
}

#[cfg(target_os = "macos")]
fn run_macos(
    cli: &Cli,
    matcher: Arc<Mutex<Matcher>>,
    rules_loaded: Arc<Mutex<usize>>,
    config_path: Arc<PathBuf>,
    aliases: Arc<HashMap<String, AppAlias>>,
) -> Result<()> {
    use sledge_macos::MacOsBackend;

    // The backend is used for both `run` (consumes &mut self on main
    // thread) and for `inject` (from the matcher sink). We keep one
    // instance for run and use a separate throwaway for inject; both paths
    // go through stateless FFI calls, so this is safe.
    let sink_backend = Arc::new(MacOsBackend::new());
    let mut run_backend = MacOsBackend::new();

    let sink: Box<dyn EventSink> = Box::new(MatcherSink {
        matcher: matcher.clone(),
        aliases: aliases.clone(),
        backend: sink_backend,
    });

    // IPC server on a background Tokio runtime.
    let socket_path = cli
        .socket
        .clone()
        .unwrap_or_else(logging::default_socket_path);
    let started_at = Instant::now();
    let matcher_for_reload = matcher.clone();
    let rules_for_reload = rules_loaded.clone();
    let config_path_for_reload = config_path.clone();

    let reload_fn: Arc<dyn Fn() -> Result<(), String> + Send + Sync> = Arc::new(move || {
        let cfg =
            sledge_config::load_from_file(&config_path_for_reload).map_err(|e| e.to_string())?;
        matcher_for_reload.lock().swap_rules(cfg.rules.clone());
        *rules_for_reload.lock() = cfg.rules.len();
        info!("config reloaded");
        Ok(())
    });

    let ipc_state = Arc::new(ServerState {
        started_at,
        rules_loaded: rules_loaded.clone(),
        focused_app: Arc::new(|| None),
        reload: reload_fn.clone(),
        check_permissions: Arc::new(|| {
            let p = sledge_macos::check_permissions();
            StatusPermissions {
                accessibility: p.accessibility,
                input_monitoring: p.input_monitoring,
            }
        }),
    });

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(2)
        .build()?;
    let listener = ipc::bind(&socket_path).context("binding IPC socket")?;
    let ipc_state_clone = ipc_state.clone();
    rt.spawn(async move { ipc::serve(listener, ipc_state_clone).await });

    install_signal_handler(reload_fn);

    run_backend.run(sink).map_err(anyhow::Error::from)?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn install_signal_handler(reload_fn: Arc<dyn Fn() -> Result<(), String> + Send + Sync>) {
    std::thread::Builder::new()
        .name("sledge-signals".into())
        .spawn(move || {
            use signal_hook::consts::{SIGHUP, SIGTERM};
            use signal_hook::iterator::Signals;
            let mut signals = match Signals::new([SIGHUP, SIGTERM]) {
                Ok(s) => s,
                Err(e) => {
                    warn!(error = %e, "failed to install signal handler");
                    return;
                }
            };
            for sig in signals.forever() {
                match sig {
                    SIGHUP => match (reload_fn)() {
                        Ok(()) => info!("reload via SIGHUP"),
                        Err(e) => warn!(error = %e, "SIGHUP reload failed"),
                    },
                    SIGTERM => {
                        info!("SIGTERM received; exiting");
                        std::process::exit(0);
                    }
                    _ => {}
                }
            }
        })
        .expect("spawn signal thread");
}

fn resolve_config_path(cli: &Cli) -> Result<PathBuf> {
    if let Some(p) = &cli.config {
        return Ok(p.clone());
    }
    if let Ok(env) = std::env::var("SLEDGE_CONFIG") {
        return Ok(PathBuf::from(env));
    }
    logging::default_config_path().context("no default config path available")
}

#[cfg(target_os = "macos")]
struct MatcherSink {
    matcher: Arc<Mutex<Matcher>>,
    aliases: Arc<HashMap<String, AppAlias>>,
    backend: Arc<sledge_macos::MacOsBackend>,
}

#[cfg(target_os = "macos")]
impl EventSink for MatcherSink {
    fn on_event(&mut self, event: KeyEvent, focused_app: Option<&str>) -> BackendVerdict {
        let logical = focused_app.and_then(|bid| resolve_logical(&self.aliases, bid));
        let app_for_match = logical.as_deref().or(focused_app);

        let verdict = self
            .matcher
            .lock()
            .dispatch(event, app_for_match, Instant::now());

        match verdict {
            Verdict::Pass => BackendVerdict::Pass,
            Verdict::Swallow => BackendVerdict::Swallow,
            Verdict::Replace(action) => {
                if let Err(e) = self.backend.inject(&action) {
                    warn!(error = %e, "inject failed");
                }
                BackendVerdict::Swallow
            }
        }
    }
}

fn resolve_logical(aliases: &HashMap<String, AppAlias>, bundle_id: &str) -> Option<String> {
    for (name, a) in aliases {
        if a.macos.as_deref() == Some(bundle_id) {
            return Some(name.clone());
        }
    }
    None
}
