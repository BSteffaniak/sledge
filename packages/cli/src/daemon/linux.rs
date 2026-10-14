//! Conservative Linux lifecycle: no device access or authorization bypasses.
use super::{MatcherSink, MatchingState, ReloadFn, reload_callback};
use crate::cli::Cli;
use crate::ipc::{self, ServerState};
use crate::logging;
use anyhow::{Context, Result};
use parking_lot::Mutex;
use sledge_core::{EventSink, InputBackend};
use sledge_linux::LinuxBackend;
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
    // Metadata/catalogue verification is off the native callback and precedes
    // desktop workers or socket admission. Candidate never implies host support.
    let installed_metadata = super::hyprland_installed::prepare(args)?;
    if let Some(metadata) = &installed_metadata {
        let config = sledge_config::load_from_file(&config_path)?;
        super::hyprland_installed::validate_rules(&config, metadata.feature_mask())?;
        let mut state = matcher.lock();
        state.replace(&config);
        state.hyprland_down_only = true;
        state.hyprland_features = metadata.feature_mask();
        state.hyprland.set_modifier_tap_release(
            metadata.feature_mask() == sledge_linux::hyprland::catalogue::TAP_FEATURES,
        );
    }
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
    let research = super::hyprland_research::ResearchListener::bind(
        &args.linux_hyprland_research,
        matcher.clone(),
        sources.clone(),
    )?;
    let research_configured = research.is_some();
    let installed = installed_metadata
        .map(|metadata| {
            super::hyprland_installed::InstalledListener::bind(
                args,
                metadata,
                matcher.clone(),
                sources.clone(),
            )
        })
        .transpose()?;
    let qualification = installed
        .as_ref()
        .map(super::hyprland_installed::InstalledListener::qualification);
    let candidate_configured =
        qualification == Some(sledge_linux::hyprland::session::Qualification::Candidate);
    let supported_configured =
        qualification == Some(sledge_linux::hyprland::session::Qualification::Supported);
    let sink: Box<dyn EventSink> = Box::new(MatcherSink {
        matcher: matcher.clone(),
        backend: inject_backend,
        sources,
        focus: desktop_focus.clone(),
    });
    let last_reload_at = Arc::new(Mutex::new(None));
    let status_matching = matcher.clone();
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
            let matching = status_matching.lock();
            let health = matching.hyprland_health;
            let operational = health.is_some_and(|health| {
                health.operational(matching.hyprland.is_reconciled(health.nonce))
            });

            Some(serde_json::json!({
                "broker_connected": caps.broker_connected,
                "verified_session": caps.verified_session,
                "source_switching": sources_configured,
                "native_send_key": false,
                "strict_lock_authority": false,
                "hyprland_research_configured": research_configured,
                "hyprland_capture_qualified": supported_configured,
                "hyprland_candidate_configured": candidate_configured,
                "hyprland_supported_configured": supported_configured,
                "hyprland_operational": operational,
                "hyprland_authenticated": health.is_some_and(|h| h.connected && h.process_is_current()),
                "hyprland_awaiting_reconciliation": health.is_some_and(|h| h.awaiting_reconciliation),
                "hyprland_locked": health.is_some_and(|h| h.locked),
                "hyprland_connection_nonce": health.map(|h| h.nonce),
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
    let stopped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let signal_stop = stopped.clone();
    let _signals = SignalThread::spawn_with(reload.clone(), move || {
        signal_stop.store(true, std::sync::atomic::Ordering::Release);
        shutdown.shutdown();
    })?;
    let _watcher = crate::config_watcher::spawn((*config_path).clone(), reload);
    // The backend's fail-closed authorization remains authoritative. This CLI
    // does not open devices, connect a privileged broker, or invent permissions.
    let result = match (installed, research) {
        (Some(listener), _) => listener.run(&stopped),
        (None, Some(listener)) => listener.run(&stopped),
        (None, None) => backend.run(sink).map_err(anyhow::Error::from),
    };
    server.abort();
    runtime.shutdown_timeout(Duration::from_secs(5));
    drop(source_work);
    drop(desktop);
    result
}

impl<B: InputBackend> MatcherSink<B> {
    pub(super) fn deliver(
        &self,
        device: sledge_core::DeviceId,
        event: sledge_core::KeyEvent,
        focused_app: Option<&str>,
    ) -> sledge_core::BackendVerdict {
        let focus = self.focus.as_ref().and_then(|cache| cache.current());
        let identity = focus.as_ref().map(|focus| match focus.category {
            sledge_linux::desktop::FocusCategory::Wayland => {
                sledge_config::AppIdentity::LinuxWayland(&focus.value)
            }
            sledge_linux::desktop::FocusCategory::X11Class => {
                sledge_config::AppIdentity::LinuxX11Class(&focus.value)
            }
            sledge_linux::desktop::FocusCategory::KwinResourceClass => {
                sledge_config::AppIdentity::LinuxLegacy(&focus.value)
            }
        });
        let (focused_app, identity) = if self.focus.is_some() {
            (focus.as_ref().map(|focus| focus.value.as_str()), identity)
        } else {
            (
                focused_app,
                focused_app.map(sledge_config::AppIdentity::LinuxLegacy),
            )
        };
        // Current broker callbacks and injection/admission are synchronous.
        // Hold the matching-state lock through settlement so reload/reset or
        // another physical event cannot interleave before the backend verdict.
        let mut state = self.matcher.lock();
        let MatchingState {
            matcher, aliases, ..
        } = &mut *state;
        let logical = super::resolve_focused_app(aliases, focused_app, identity);
        let (verdict, token) =
            matcher.dispatch_device_with_delivery(device, event, logical, Instant::now());
        match verdict {
            sledge_core::Verdict::Pass => sledge_core::BackendVerdict::Pass,
            sledge_core::Verdict::Swallow => sledge_core::BackendVerdict::Swallow,
            sledge_core::Verdict::Replace(action) => {
                let result = if let sledge_core::Action::SetInputSource { id } = &action {
                    self.sources
                        .as_ref()
                        .ok_or_else(|| sledge_core::BackendError::UnknownInputSource(id.clone()))
                        .and_then(|queue| queue.submit(id.clone()))
                } else {
                    self.backend.inject(&action)
                };
                let (delivery, verdict) = match result {
                    Ok(()) => (
                        // Queue admission consumes the original under existing
                        // semantics; it is NOT proof of remote source completion.
                        sledge_core::ReplacementDelivery::Delivered,
                        sledge_core::BackendVerdict::Swallow,
                    ),
                    Err(error) => {
                        warn!(%error, "inject failed");
                        // Current Linux SendKey is unsupported before output;
                        // SourceQueue errors are pre-admission only. Returning
                        // Pass here is the broker's original-forwarding decision.
                        // Do not extend this to timeout/partial-output errors.
                        (
                            sledge_core::ReplacementDelivery::RejectedOriginalForwarded,
                            sledge_core::BackendVerdict::Pass,
                        )
                    }
                };
                matcher.report_delivery(token.expect("replacement token"), delivery);
                verdict
            }
        }
    }
}

struct SignalThread {
    handle: signal_hook::iterator::Handle,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl SignalThread {
    fn spawn_with(reload: ReloadFn, shutdown: impl Fn() + Send + 'static) -> Result<Self> {
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
                        shutdown();
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

#[cfg(test)]
mod tests {
    use super::*;
    use sledge_core::{
        Action, BackendError, BackendVerdict, DeviceId, EventKind, HotkeyTrigger, KeyCode,
        KeyEvent, Modifiers, Rule, RuleSet, TapTrigger, Trigger,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct SelectiveBackend(AtomicUsize);

    impl InputBackend for SelectiveBackend {
        fn run(&mut self, _sink: Box<dyn EventSink>) -> Result<(), BackendError> {
            Ok(())
        }

        fn inject(&self, action: &Action) -> Result<(), BackendError> {
            self.0.fetch_add(1, Ordering::Relaxed);
            if matches!(
                action,
                Action::SendKey {
                    key: KeyCode::Return,
                    ..
                }
            ) {
                Ok(())
            } else {
                Err(BackendError::Inject("unsupported before output".into()))
            }
        }
    }

    fn send(key: KeyCode) -> Action {
        Action::SendKey {
            key,
            mods: Modifiers::empty(),
        }
    }

    fn rules(failed_action: Action) -> RuleSet {
        let hotkey = |key, action| Rule {
            trigger: Trigger::Hotkey(HotkeyTrigger {
                key,
                mods: Modifiers::empty(),
            }),
            action,
            when_app_in: None,
        };
        let tap = |count| Rule {
            trigger: Trigger::Tap(TapTrigger {
                tap: KeyCode::Fn,
                count,
                within_ms: 600,
            }),
            action: send(KeyCode::Return),
            when_app_in: None,
        };
        RuleSet::new(vec![
            hotkey(KeyCode::KeyA, failed_action),
            hotkey(KeyCode::KeyK, send(KeyCode::Return)),
            tap(2),
            tap(3),
        ])
    }

    fn route(
        sink: &mut MatcherSink<SelectiveBackend>,
        device: u64,
        key: KeyCode,
        kind: EventKind,
    ) -> BackendVerdict {
        sink.on_device_event(
            DeviceId(device),
            KeyEvent {
                code: key,
                kind,
                mods: Modifiers::empty(),
            },
            None,
        )
    }

    #[test]
    fn precommit_failures_keep_unrelated_consumed_press_and_modifier_tap_sequence() {
        for failed_action in [
            send(KeyCode::KeyB),
            Action::SetInputSource {
                id: "missing".into(),
            },
        ] {
            let config = sledge_config::parse_str("").unwrap();
            let mut state = MatchingState::new(&config);
            state.matcher.swap_rules(rules(failed_action));
            let backend = Arc::new(SelectiveBackend(AtomicUsize::new(0)));
            let mut sink = MatcherSink {
                matcher: Arc::new(Mutex::new(state)),
                backend: backend.clone(),
                sources: None,
                focus: None,
            };
            assert!(matches!(
                route(&mut sink, 1, KeyCode::KeyK, EventKind::KeyDown),
                BackendVerdict::Swallow
            ));
            // A separate keyboard's second modifier tap is consumed. Its third
            // tap must still fire after the failed replacement on keyboard 1.
            for kind in [EventKind::KeyDown, EventKind::KeyUp, EventKind::KeyDown] {
                assert!(matches!(
                    route(&mut sink, 2, KeyCode::Fn, kind),
                    BackendVerdict::Pass
                ));
            }
            assert!(matches!(
                route(&mut sink, 2, KeyCode::Fn, EventKind::KeyUp),
                BackendVerdict::Swallow
            ));
            assert!(matches!(
                route(&mut sink, 2, KeyCode::Fn, EventKind::KeyDown),
                BackendVerdict::Pass
            ));
            assert!(matches!(
                route(&mut sink, 1, KeyCode::KeyA, EventKind::KeyDown),
                BackendVerdict::Pass
            ));
            assert!(matches!(
                route(&mut sink, 2, KeyCode::Fn, EventKind::KeyUp),
                BackendVerdict::Swallow
            ));
            let attempts = backend.0.load(Ordering::Relaxed);
            // Reload does not discard either consumed or forwarded ownership.
            sink.matcher.lock().replace(&config);
            assert!(matches!(
                route(&mut sink, 1, KeyCode::KeyK, EventKind::KeyDown),
                BackendVerdict::Swallow
            ));
            assert!(matches!(
                route(&mut sink, 1, KeyCode::KeyK, EventKind::KeyUp),
                BackendVerdict::Swallow
            ));
            for kind in [EventKind::KeyDown, EventKind::KeyUp] {
                assert!(matches!(
                    route(&mut sink, 1, KeyCode::KeyA, kind),
                    BackendVerdict::Pass
                ));
            }
            assert_eq!(backend.0.load(Ordering::Relaxed), attempts);
            assert!(matches!(
                route(&mut sink, 1, KeyCode::KeyK, EventKind::KeyDown),
                BackendVerdict::Pass
            ));
        }
    }

    #[test]
    fn device_and_stream_reset_retire_only_core_delivery_ownership() {
        let config = sledge_config::parse_str("").unwrap();
        let mut state = MatchingState::new(&config);
        state.matcher.swap_rules(rules(send(KeyCode::KeyB)));
        let mut sink = MatcherSink {
            matcher: Arc::new(Mutex::new(state)),
            backend: Arc::new(SelectiveBackend(AtomicUsize::new(0))),
            sources: None,
            focus: None,
        };
        assert!(matches!(
            route(&mut sink, 1, KeyCode::KeyA, EventKind::KeyDown),
            BackendVerdict::Pass
        ));
        for device in [2, 3] {
            assert!(matches!(
                route(&mut sink, device, KeyCode::KeyK, EventKind::KeyDown),
                BackendVerdict::Swallow
            ));
        }
        sink.on_device_reset(DeviceId(2));
        assert!(matches!(
            route(&mut sink, 2, KeyCode::KeyK, EventKind::KeyUp),
            BackendVerdict::Pass
        ));
        assert!(matches!(
            route(&mut sink, 3, KeyCode::KeyK, EventKind::KeyUp),
            BackendVerdict::Swallow
        ));
        assert!(matches!(
            route(&mut sink, 1, KeyCode::KeyA, EventKind::KeyDown),
            BackendVerdict::Pass
        ));
        let attempts = sink.backend.0.load(Ordering::Relaxed);
        sink.on_stream_reset();
        assert!(matches!(
            route(&mut sink, 1, KeyCode::KeyA, EventKind::KeyDown),
            BackendVerdict::Pass
        ));
        assert_eq!(sink.backend.0.load(Ordering::Relaxed), attempts + 1);
    }
}
