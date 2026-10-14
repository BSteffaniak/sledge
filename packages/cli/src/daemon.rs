//! Daemon main loop: wire backend + matcher + IPC, handle signals.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use parking_lot::Mutex;
use sledge_config::models::AppAlias;
#[cfg(any(not(target_os = "linux"), test))]
use sledge_core::Verdict;
use sledge_core::{BackendVerdict, DeviceId, EventSink, InputBackend, KeyEvent, Matcher};
use tracing::{info, warn};

use crate::cli::{Cli, RunArgs};
#[cfg(target_os = "macos")]
use crate::ipc::StatusPermissions;
#[cfg(target_os = "macos")]
use crate::ipc::{self, ServerState};
use crate::logging;

#[cfg(target_os = "linux")]
mod desktop;
#[cfg(target_os = "linux")]
mod hyprland_installed;
mod hyprland_research;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
mod source_queue;
#[cfg(target_os = "linux")]
use linux::run_linux;

/// Rules and aliases share one lock so an event never observes mixed config
/// generations during a reload.
struct MatchingState {
    matcher: Matcher,
    aliases: HashMap<String, AppAlias>,
    #[cfg(target_os = "linux")]
    hyprland: sledge_linux::hyprland::matcher::MatcherTransactions,
    #[cfg(target_os = "linux")]
    hyprland_down_only: bool,
    #[cfg(target_os = "linux")]
    hyprland_features: u64,
    #[cfg(target_os = "linux")]
    hyprland_health: Option<sledge_linux::hyprland::session::SessionHealth>,
}

impl MatchingState {
    fn new(config: &sledge_config::Config) -> Self {
        Self {
            matcher: Matcher::new(config.rules.clone()),
            aliases: config.app_aliases.clone(),
            #[cfg(target_os = "linux")]
            hyprland: sledge_linux::hyprland::matcher::MatcherTransactions::default(),
            #[cfg(target_os = "linux")]
            hyprland_down_only: false,
            #[cfg(target_os = "linux")]
            hyprland_features: 1,
            #[cfg(target_os = "linux")]
            hyprland_health: None,
        }
    }

    fn replace(&mut self, config: &sledge_config::Config) {
        #[cfg(target_os = "linux")]
        self.hyprland.invalidate_speculation(&mut self.matcher);
        self.matcher.swap_rules(config.rules.clone());
        self.aliases.clone_from(&config.app_aliases);
    }

    #[cfg(test)]
    fn dispatch(&mut self, event: KeyEvent, focused_app: Option<&str>) -> Verdict {
        self.dispatch_device(DeviceId(0), event, focused_app)
    }

    #[cfg(any(not(target_os = "linux"), test))]
    fn dispatch_device(
        &mut self,
        device: DeviceId,
        event: KeyEvent,
        focused_app: Option<&str>,
    ) -> Verdict {
        self.dispatch_identity(device, event, focused_app, focused_app.map(app_identity))
    }

    #[cfg(any(not(target_os = "linux"), test))]
    fn dispatch_identity(
        &mut self,
        device: DeviceId,
        event: KeyEvent,
        focused_app: Option<&str>,
        identity: Option<sledge_config::AppIdentity<'_>>,
    ) -> Verdict {
        let logical = resolve_focused_app(&self.aliases, focused_app, identity);
        self.matcher
            .dispatch_device(device, event, logical, Instant::now())
    }
}

fn resolve_focused_app<'a>(
    aliases: &'a HashMap<String, AppAlias>,
    focused_app: Option<&'a str>,
    identity: Option<sledge_config::AppIdentity<'_>>,
) -> Option<&'a str> {
    let logical = identity.and_then(|id| sledge_config::resolve_app_identity(aliases, id));
    #[cfg(target_os = "linux")]
    let logical = logical.or_else(|| {
        focused_app.and_then(|id| {
            sledge_config::resolve_app_identity(
                aliases,
                sledge_config::AppIdentity::LinuxLegacy(id),
            )
        })
    });
    logical.or(focused_app)
}

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

    let guard =
        logging::init(&initial.daemon.log_level, args.stdout_logs).context("tracing init")?;
    let reload_handle = guard.reload_handle();

    info!(
        version = env!("CARGO_PKG_VERSION"),
        config = %config_path.display(),
        "sledge starting"
    );

    let matcher = Arc::new(Mutex::new(MatchingState::new(&initial)));
    let rules_loaded = Arc::new(Mutex::new(initial.rules.len()));
    let config_path = Arc::new(config_path);

    #[cfg(target_os = "macos")]
    {
        // Keep `guard` alive for the lifetime of the daemon so file-log
        // flushing works on shutdown.
        let _guard = guard;
        run_macos(cli, matcher, rules_loaded, config_path, reload_handle)
    }
    #[cfg(target_os = "linux")]
    {
        let _guard = guard;
        run_linux(
            cli,
            &args,
            matcher,
            rules_loaded,
            config_path,
            reload_handle,
        )
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (matcher, rules_loaded, config_path, reload_handle);
        drop(guard);
        anyhow::bail!("only the macOS backend is implemented")
    }
}

#[cfg(target_os = "macos")]
fn run_macos(
    cli: &Cli,
    matcher: Arc<Mutex<MatchingState>>,
    rules_loaded: Arc<Mutex<usize>>,
    config_path: Arc<PathBuf>,
    reload_handle: logging::FilterReloadHandle,
) -> Result<()> {
    use sledge_macos::MacOsBackend;

    // The backend is used for both `run` (consumes &mut self on main
    // thread) and for `inject` (from the matcher sink). We keep one
    // instance for run and use a separate throwaway for inject; both paths
    // go through stateless FFI calls, so this is safe.
    let sink_backend = Arc::new(MacOsBackend::new());
    let mut run_backend = MacOsBackend::new();
    // Share the focus tracker with the IPC server so `sledge status` can
    // report the currently-focused application. The tracker is populated
    // by the polling thread owned by `run_backend.run(...)`.
    let focus_tracker = run_backend.focus_tracker();

    let sink: Box<dyn EventSink> = Box::new(MatcherSink {
        matcher: matcher.clone(),
        backend: sink_backend,
    });

    // IPC server on a background Tokio runtime.
    let socket_path = cli
        .socket
        .clone()
        .unwrap_or_else(logging::default_socket_path);
    let started_at = Instant::now();
    let last_reload_at: Arc<Mutex<Option<Instant>>> = Arc::new(Mutex::new(None));
    let reload_fn = reload_callback(
        matcher.clone(),
        rules_loaded.clone(),
        config_path.clone(),
        last_reload_at.clone(),
        reload_handle,
    );

    let focus_for_ipc = focus_tracker.clone();
    let ipc_state = Arc::new(ServerState {
        started_at,
        rules_loaded: rules_loaded.clone(),
        focused_app: Arc::new(move || focus_for_ipc.current()),
        reload: reload_fn.clone(),
        check_permissions: Arc::new(|| {
            let p = sledge_macos::check_permissions();
            Some(StatusPermissions {
                accessibility: p.accessibility,
                input_monitoring: p.input_monitoring,
            })
        }),
        capabilities: Arc::new(|| None),
        last_reload_at: last_reload_at.clone(),
    });

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(2)
        .build()?;
    // `UnixListener::bind` registers the FD with the current Tokio
    // reactor; enter the runtime briefly so it has one to register with.
    let listener = {
        let _guard = rt.enter();
        ipc::bind(&socket_path).context("binding IPC socket")?
    };
    let ipc_state_clone = ipc_state.clone();
    rt.spawn(async move { ipc::serve(listener, ipc_state_clone).await });

    install_signal_handler(reload_fn.clone());

    // Spawn the config-file watcher. Held for the lifetime of the daemon;
    // drop stops the watcher thread.
    let _config_watcher = crate::config_watcher::spawn((*config_path).clone(), reload_fn);

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

type ReloadFn = Arc<dyn Fn() -> Result<(), String> + Send + Sync>;

fn reload_callback(
    matcher: Arc<Mutex<MatchingState>>,
    rules_loaded: Arc<Mutex<usize>>,
    config_path: Arc<PathBuf>,
    last_reload_at: Arc<Mutex<Option<Instant>>>,
    reload_handle: logging::FilterReloadHandle,
) -> ReloadFn {
    // Serialize parsing too: an older overlapping reload cannot publish after
    // a newer generation from IPC, SIGHUP or the file watcher.
    let reload_lock = Mutex::new(());
    Arc::new(move || {
        let _reload = reload_lock.lock();
        let cfg = sledge_config::load_from_file(&config_path).map_err(|e| e.to_string())?;
        let mut state = matcher.lock();
        #[cfg(target_os = "linux")]
        if state.hyprland_down_only {
            hyprland_installed::validate_rules(&cfg, state.hyprland_features)
                .map_err(|e| e.to_string())?;
        }
        state.replace(&cfg);
        drop(state);
        *rules_loaded.lock() = cfg.rules.len();
        *last_reload_at.lock() = Some(Instant::now());
        info!("config reloaded");
        if let Err(e) = logging::apply_level(&reload_handle, &cfg.daemon.log_level) {
            warn!(error = %e, "log level change failed");
        }
        Ok(())
    })
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

struct MatcherSink<B> {
    matcher: Arc<Mutex<MatchingState>>,
    backend: Arc<B>,
    #[cfg(target_os = "linux")]
    sources: Option<source_queue::SourceQueue>,
    #[cfg(target_os = "linux")]
    focus: Option<sledge_linux::desktop::worker::FocusCache>,
}

impl<B: InputBackend + Sync> EventSink for MatcherSink<B> {
    fn on_event(&mut self, event: KeyEvent, focused_app: Option<&str>) -> BackendVerdict {
        self.deliver(DeviceId(0), event, focused_app)
    }

    fn on_device_event(
        &mut self,
        device: DeviceId,
        event: KeyEvent,
        focused_app: Option<&str>,
    ) -> BackendVerdict {
        self.deliver(device, event, focused_app)
    }

    fn on_device_reset(&mut self, device: DeviceId) {
        let mut state = self.matcher.lock();
        state.matcher.reset_device(device);
    }

    fn on_stream_reset(&mut self) {
        let mut state = self.matcher.lock();
        state.matcher.reset();
    }
}

#[cfg(not(target_os = "linux"))]
impl<B: InputBackend> MatcherSink<B> {
    fn deliver(
        &self,
        device: DeviceId,
        event: KeyEvent,
        focused_app: Option<&str>,
    ) -> BackendVerdict {
        let verdict = self
            .matcher
            .lock()
            .dispatch_device(device, event, focused_app);
        self.apply(verdict)
    }

    fn apply(&self, verdict: Verdict) -> BackendVerdict {
        match verdict {
            Verdict::Pass => BackendVerdict::Pass,
            Verdict::Swallow => BackendVerdict::Swallow,
            Verdict::Replace(action) => {
                let result = self.backend.inject(&action);
                if let Err(e) = result {
                    warn!(error = %e, "inject failed");
                }
                BackendVerdict::Swallow
            }
        }
    }
}

#[cfg(any(not(target_os = "linux"), test))]
fn app_identity(id: &str) -> sledge_config::AppIdentity<'_> {
    #[cfg(target_os = "macos")]
    {
        sledge_config::AppIdentity::MacOS(id)
    }
    #[cfg(target_os = "linux")]
    {
        sledge_config::AppIdentity::LinuxLegacy(id)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        sledge_config::AppIdentity::Windows(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sledge_core::{EventKind, KeyCode, Modifiers};

    fn config(bundle: &str) -> sledge_config::Config {
        let platform_key = match sledge_config::Platform::current() {
            sledge_config::Platform::MacOS => "macos",
            sledge_config::Platform::Linux => "linux",
            sledge_config::Platform::Windows => "windows",
        };
        sledge_config::parse_str(&format!(
            r#"
[app_aliases.editor]
{platform_key} = "{bundle}"
[[binding]]
when = {{ app_in = ["editor"] }}
trigger = {{ key = "KeyK", mods = ["ctrl"] }}
action = {{ type = "send_key", key = "Return", mods = [] }}
"#
        ))
        .expect("valid config")
    }

    #[cfg(target_os = "linux")]
    struct RejectingBackend;

    #[cfg(target_os = "linux")]
    impl InputBackend for RejectingBackend {
        fn run(&mut self, _sink: Box<dyn EventSink>) -> Result<(), sledge_core::BackendError> {
            Ok(())
        }

        fn inject(&self, _action: &sledge_core::Action) -> Result<(), sledge_core::BackendError> {
            Err(sledge_core::BackendError::Inject("unsupported".into()))
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn unsupported_replacement_preserves_original_linux_event() {
        let mut sink = MatcherSink {
            matcher: Arc::new(Mutex::new(MatchingState::new(&config("editor.app")))),
            backend: Arc::new(RejectingBackend),
            sources: None,
            focus: None,
        };
        let down = KeyEvent {
            code: KeyCode::KeyK,
            kind: EventKind::KeyDown,
            mods: Modifiers::CTRL,
        };
        assert!(matches!(
            sink.on_event(down, Some("editor.app")),
            BackendVerdict::Pass
        ));
        // Repeat and release must follow the disposition of the forwarded down,
        // even if scope or configuration changes before the release.
        sink.matcher.lock().replace(&config("other.app"));
        assert!(matches!(
            sink.on_event(down, Some("other.app")),
            BackendVerdict::Pass
        ));
        let up = KeyEvent {
            kind: EventKind::KeyUp,
            ..down
        };
        assert!(matches!(sink.on_event(up, None), BackendVerdict::Pass));
        assert!(matches!(
            sink.on_device_event(DeviceId(7), down, Some("other.app")),
            BackendVerdict::Pass
        ));
        assert!(matches!(
            sink.on_device_event(DeviceId(7), up, None),
            BackendVerdict::Pass
        ));
    }

    struct AcceptingBackend;

    impl InputBackend for AcceptingBackend {
        fn run(&mut self, _sink: Box<dyn EventSink>) -> Result<(), sledge_core::BackendError> {
            Ok(())
        }

        fn inject(&self, _action: &sledge_core::Action) -> Result<(), sledge_core::BackendError> {
            Ok(())
        }
    }

    #[test]
    fn device_sink_preserves_reload_ownership_and_isolates_resets() {
        let state = Arc::new(Mutex::new(MatchingState::new(&config("editor.app"))));
        let mut sink = MatcherSink {
            matcher: state.clone(),
            backend: Arc::new(AcceptingBackend),
            #[cfg(target_os = "linux")]
            sources: None,
            #[cfg(target_os = "linux")]
            focus: None,
        };
        let down = KeyEvent {
            code: KeyCode::KeyK,
            kind: EventKind::KeyDown,
            mods: Modifiers::CTRL,
        };
        let up = KeyEvent {
            kind: EventKind::KeyUp,
            ..down
        };
        for device in [DeviceId(1), DeviceId(2)] {
            assert!(matches!(
                sink.on_device_event(device, down, Some("editor.app")),
                BackendVerdict::Swallow
            ));
        }
        state.lock().replace(&sledge_config::parse_str("").unwrap());
        assert!(matches!(
            sink.on_device_event(DeviceId(3), up, None),
            BackendVerdict::Pass
        ));
        sink.on_device_reset(DeviceId(1));
        assert!(matches!(
            sink.on_device_event(DeviceId(1), up, None),
            BackendVerdict::Pass
        ));
        assert!(matches!(
            sink.on_device_event(DeviceId(2), up, None),
            BackendVerdict::Swallow
        ));

        state.lock().replace(&config("editor.app"));
        assert!(matches!(
            sink.on_device_event(DeviceId(2), down, Some("editor.app")),
            BackendVerdict::Swallow
        ));
        sink.on_stream_reset();
        assert!(matches!(
            sink.on_device_event(DeviceId(2), up, None),
            BackendVerdict::Pass
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn typed_focus_resolution_does_not_guess_kwin_protocol() {
        let config = sledge_config::parse_str(
            r#"
[app_aliases.editor]
linux_ids = { wayland = "same", x11_class = "xclass" }
[[binding]]
when = { app_in = ["editor"] }
trigger = { key = "KeyK", mods = ["ctrl"] }
action = { type = "send_key", key = "Return", mods = [] }
"#,
        )
        .unwrap();
        let mut state = MatchingState::new(&config);
        let down = KeyEvent {
            code: KeyCode::KeyK,
            kind: EventKind::KeyDown,
            mods: Modifiers::CTRL,
        };
        assert!(matches!(
            state.dispatch_identity(
                DeviceId(1),
                down,
                Some("same"),
                Some(sledge_config::AppIdentity::LinuxWayland("same"))
            ),
            Verdict::Replace(_)
        ));
        assert!(matches!(
            state.dispatch_identity(
                DeviceId(2),
                down,
                Some("same"),
                Some(sledge_config::AppIdentity::LinuxLegacy("same"))
            ),
            Verdict::Pass
        ));
        assert!(matches!(
            state.dispatch_identity(DeviceId(3), down, None, None),
            Verdict::Pass
        ));
        assert!(matches!(
            state.dispatch_identity(
                DeviceId(4),
                down,
                Some("xclass"),
                Some(sledge_config::AppIdentity::LinuxX11Class("xclass"))
            ),
            Verdict::Replace(_)
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn source_id_is_committed_by_matching_generation_before_reload() {
        let config = |id| {
            sledge_config::parse_str(&format!(
                r#"
[input_source_aliases.editor]
linux = "{id}"
[[binding]]
trigger = {{ key = "KeyK", mods = ["ctrl"] }}
action = {{ type = "set_input_source", source = "editor" }}
"#
            ))
            .unwrap()
        };
        let mut state = MatchingState::new(&config("native-old"));
        let event = KeyEvent {
            code: KeyCode::KeyK,
            kind: EventKind::KeyDown,
            mods: Modifiers::CTRL,
        };
        let committed = state.dispatch_device(DeviceId(1), event, None);
        state.replace(&config("native-new"));
        assert!(
            matches!(committed, Verdict::Replace(sledge_core::Action::SetInputSource { id }) if id == "native-old")
        );
        assert!(
            matches!(state.dispatch_device(DeviceId(2), event, None), Verdict::Replace(sledge_core::Action::SetInputSource { id }) if id == "native-new")
        );
    }

    #[test]
    fn reload_replaces_aliases_together_with_rules() {
        let mut state = MatchingState::new(&config("old.editor"));
        let event = KeyEvent {
            code: KeyCode::KeyK,
            kind: EventKind::KeyDown,
            mods: Modifiers::CTRL,
        };
        assert!(matches!(
            state.dispatch(event, Some("old.editor")),
            Verdict::Replace(_)
        ));
        state.dispatch(
            KeyEvent {
                kind: EventKind::KeyUp,
                ..event
            },
            Some("old.editor"),
        );
        state.replace(&config("new.editor"));
        assert!(matches!(
            state.dispatch(event, Some("old.editor")),
            Verdict::Pass
        ));
        assert!(matches!(
            state.dispatch(event, Some("new.editor")),
            Verdict::Replace(_)
        ));
    }
}
