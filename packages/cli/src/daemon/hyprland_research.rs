//! Explicit isolated research bridge; no production capture or socket discovery.
use super::{MatchingState, resolve_focused_app, source_queue::SourceQueue};
use crate::cli::HyprlandResearchArgs;
use anyhow::{Context, Result};
use parking_lot::Mutex;
use sledge_linux::hyprland::listener::Listener;
use sledge_linux::hyprland::matcher::MatcherAdapter;
use sledge_linux::hyprland::transport::{Peer, monotonic_ns};
use sledge_linux::hyprland::wire::{Body, Decision, Event, Identity, Scope};
use sledge_linux::hyprland::{Bridge, DeliveryReport, EventHandler};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Instant;

pub(super) struct ResearchListener {
    listener: Listener,
    peer: Peer,
    identity: Identity,
    handler: Handler,
}
impl ResearchListener {
    pub(super) fn bind(
        args: &HyprlandResearchArgs,
        matching: Arc<Mutex<MatchingState>>,
        sources: Option<SourceQueue>,
    ) -> Result<Option<Self>> {
        let Some(path) = &args.linux_hyprland_research_socket else {
            return Ok(None);
        };
        let pid = args
            .linux_hyprland_research_pid
            .context("research compositor PID required")?;
        let api_hash = args
            .linux_hyprland_research_api_hash
            .clone()
            .context("research API hash required")?;
        let artifact = args
            .linux_hyprland_research_artifact
            .clone()
            .context("research artifact required")?;
        anyhow::ensure!(
            pid > 0
                && [&api_hash, &artifact]
                    .iter()
                    .all(|v| !v.is_empty() && v.len() <= 192 && !v.contains('\0')),
            "invalid research identity"
        );
        // SAFETY: geteuid takes no arguments and does not change process state.
        let uid = unsafe { libc::geteuid() };
        Ok(Some(Self {
            listener: Listener::bind(path).context("binding private isolated research listener")?,
            peer: Peer { uid, pid },
            identity: Identity {
                capabilities: 0,
                api_hash,
                artifact,
            },
            handler: Handler { matching, sources },
        }))
    }

    pub(super) fn run(mut self, stopped: &AtomicBool) -> Result<()> {
        tracing::info!("isolated Hyprland research listening; capabilities=0 qualified=false");
        let admission_deadline = monotonic_ns()? + 60_000_000_000;
        let transport = loop {
            if stopped.load(Ordering::Acquire) {
                return Ok(());
            }
            let deadline = admission_deadline.min(monotonic_ns()? + 100_000_000);
            match self.listener.accept(self.peer, deadline) {
                Ok(transport) => break transport,
                Err(error)
                    if error.kind() == std::io::ErrorKind::TimedOut
                        && monotonic_ns()? < admission_deadline => {}
                Err(error) => return Err(error).context("research peer admission (no reconnect)"),
            }
        };
        let mut bridge =
            Bridge::passive_handshake(transport, &self.identity, monotonic_ns()? + 10_000_000)?;
        let nonce = bridge.connection_nonce();
        {
            let mut state = self.handler.matching.lock();
            let MatchingState {
                matcher, hyprland, ..
            } = &mut *state;
            hyprland.await_reconciliation(matcher, nonce);
        }
        while !stopped.load(Ordering::Acquire) {
            if let Err(error) =
                bridge.research_step_if_ready(&mut self.handler, monotonic_ns()? + 100_000_000)
            {
                // Preserve the same bridge/handler across admission so nonce
                // history, retired generations and committed holds survive.
                tracing::warn!(%error, "research transport lost; matching disabled until native reconciliation");
                loop {
                    if stopped.load(Ordering::Acquire) {
                        return Ok(());
                    }
                    let deadline = monotonic_ns()? + 100_000_000;
                    match self.listener.accept(self.peer, deadline) {
                        Ok(transport) => {
                            bridge.reconnect(
                                transport,
                                &self.identity,
                                monotonic_ns()? + 10_000_000,
                            )?;
                            let mut state = self.handler.matching.lock();
                            let MatchingState {
                                matcher, hyprland, ..
                            } = &mut *state;
                            hyprland.await_reconciliation(matcher, bridge.connection_nonce());
                            break;
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {}
                        Err(error) => return Err(error).context("research reconnect admission"),
                    }
                }
            }
        }
        self.handler.disconnected();
        Ok(())
    }
}

pub(super) struct Handler {
    pub(super) matching: Arc<Mutex<MatchingState>>,
    pub(super) sources: Option<SourceQueue>,
}
impl EventHandler for Handler {
    fn propose(&mut self, scope: Scope, event: &Event) -> Decision {
        // Never block a native deadline behind config reload parsing/publication.
        let Some(mut state) = self.matching.try_lock() else {
            return Decision::Pass;
        };
        let focus = MatcherAdapter::focus(event);
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
        let logical = resolve_focused_app(
            &state.aliases,
            focus.as_ref().map(|f| f.value.as_str()),
            identity,
        )
        .map(str::to_owned);
        let MatchingState {
            matcher, hyprland, ..
        } = &mut *state;
        hyprland
            .propose_with_source(
                matcher,
                scope,
                event,
                logical.as_deref(),
                Instant::now(),
                |id| {
                    self.sources
                        .as_ref()
                        .is_some_and(|queue| queue.submit(id.to_owned()).is_ok())
                },
            )
            .unwrap_or(Decision::Pass)
    }
    fn delivery(&mut self, scope: Scope, event: &Event, _: Decision, report: DeliveryReport) {
        let mut state = self.matching.lock();
        let MatchingState {
            matcher, hyprland, ..
        } = &mut *state;
        tracing::debug!(
            generation = scope.generation,
            epoch = scope.lock_epoch,
            sequence = scope.sequence,
            nonce = scope.nonce,
            key = event.key,
            state = event.state,
            focus_kind = event.focus_kind,
            ?report,
            "Hyprland native delivery outcome"
        );
        if event.state == 1 {
            hyprland.on_delivery(matcher, scope, &report);
        } else {
            hyprland.settle_device_event(matcher, scope, event, &report);
        }
    }
    fn reset(&mut self, scope: Scope, body: &Body) {
        let mut state = self.matching.lock();
        let MatchingState {
            matcher, hyprland, ..
        } = &mut *state;
        if matches!(body, Body::Reset(3)) {
            hyprland.await_lifecycle_reconciliation(matcher, scope, body);
        }
        let reconciled = hyprland.reconciled(matcher, scope, body);
        if matches!(body, Body::Reset(8)) {
            tracing::debug!(
                epoch = scope.lock_epoch,
                sequence = scope.sequence,
                nonce = scope.nonce,
                accepted = reconciled,
                "Hyprland native reconciliation"
            );
        }
        if !reconciled {
            hyprland.invalidate_speculation(matcher);
        }
        if matches!(body, Body::Retire(_)) {
            hyprland.retire_device(matcher, scope.generation);
        }
    }
    fn disconnected(&mut self) {
        let mut state = self.matching.lock();
        let MatchingState {
            matcher, hyprland, ..
        } = &mut *state;
        hyprland.disconnected(matcher);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sledge_linux::hyprland::wire::{Outcome, ResultKind};

    fn handler() -> Handler {
        let config = sledge_config::parse_str(
            r#"
[app_aliases.editor]
linux_ids = { wayland = "test.editor" }
[[binding]]
trigger = { key = "KeyA", mods = [] }
action = { type = "send_key", key = "KeyB", mods = [] }
when = { app_in = ["editor"] }
"#,
        )
        .expect("valid fixture");
        Handler {
            matching: Arc::new(Mutex::new(MatchingState::new(&config))),
            sources: None,
        }
    }
    fn scope(sequence: u64) -> Scope {
        Scope {
            generation: 1,
            lock_epoch: 1,
            sequence,
            deadline_ns: 1,
            nonce: 1,
        }
    }
    fn event(state: u32, focus_kind: u16) -> Event {
        Event {
            key: 30,
            state,
            time_ms: 1,
            modifiers: 0,
            focus_epoch: 1,
            focus_kind,
            focus: "test.editor".into(),
        }
    }
    fn commit(handler: &mut Handler) {
        let decision = handler.propose(scope(1), &event(1, 1));
        assert_eq!(
            decision,
            Decision::Replace {
                key: 48,
                modifiers: 0
            }
        );
        handler.delivery(
            scope(1),
            &event(1, 1),
            decision,
            DeliveryReport::Acknowledged(Outcome {
                result: ResultKind::Replaced,
                reason: 0,
                key: 30,
                state: 1,
                focus_epoch: 1,
            }),
        );
    }
    #[test]
    fn exact_typed_focus_resolves_alias_and_keeps_hold_across_reset() {
        let mut handler = handler();
        commit(&mut handler);
        handler.reset(scope(2), &Body::Reset(0));
        assert_eq!(handler.propose(scope(3), &event(1, 1)), Decision::Swallow);
    }
    #[test]
    fn native_release_settlement_allows_next_fresh_replacement() {
        let mut handler = handler();
        commit(&mut handler);
        handler.delivery(
            scope(2),
            &event(0, 1),
            Decision::Pass,
            DeliveryReport::Acknowledged(Outcome {
                result: ResultKind::SwallowCommitted,
                reason: 0,
                key: 30,
                state: 0,
                focus_epoch: 1,
            }),
        );
        assert_eq!(
            handler.propose(scope(3), &event(1, 1)),
            Decision::Replace {
                key: 48,
                modifiers: 0
            }
        );
    }

    #[test]
    fn wrong_focus_category_does_not_match_wayland_alias() {
        assert_eq!(handler().propose(scope(1), &event(1, 2)), Decision::Pass);
    }
    #[test]
    fn authenticated_recovery_boundary_is_required_after_disconnect() {
        let mut handler = handler();
        commit(&mut handler);
        handler.disconnected();
        {
            let mut state = handler.matching.lock();
            let MatchingState {
                matcher, hyprland, ..
            } = &mut *state;
            hyprland.await_reconciliation(matcher, 2);
        }
        let boundary = Scope {
            generation: 0,
            nonce: 2,
            ..scope(2)
        };
        handler.reset(boundary, &Body::Reset(0));
        assert_eq!(
            handler.propose(
                Scope {
                    nonce: 2,
                    ..scope(3)
                },
                &event(1, 1)
            ),
            Decision::Pass
        );
        handler.reset(
            Scope {
                nonce: 1,
                ..boundary
            },
            &Body::Reset(8),
        );
        assert_eq!(
            handler.propose(
                Scope {
                    nonce: 2,
                    ..scope(4)
                },
                &event(1, 1)
            ),
            Decision::Pass
        );
        handler.reset(boundary, &Body::Reset(8));
        assert_eq!(
            handler.propose(
                Scope {
                    nonce: 2,
                    ..scope(5)
                },
                &event(1, 1)
            ),
            Decision::Replace {
                key: 48,
                modifiers: 0
            }
        );
    }

    #[test]
    fn lifecycle_reconciliation_rearms_the_same_authenticated_session() {
        let mut handler = handler();
        {
            let mut state = handler.matching.lock();
            let MatchingState {
                matcher, hyprland, ..
            } = &mut *state;
            hyprland.await_reconciliation(matcher, 1);
        }
        let boundary = Scope {
            generation: 0,
            ..scope(1)
        };
        handler.reset(boundary, &Body::Reset(8));
        commit(&mut handler);
        handler.reset(boundary, &Body::Reset(3));
        assert_eq!(handler.propose(scope(3), &event(1, 1)), Decision::Pass);
        handler.reset(boundary, &Body::Reset(8));
        assert_eq!(
            handler.propose(scope(4), &event(1, 1)),
            Decision::Replace {
                key: 48,
                modifiers: 0
            }
        );
    }

    #[test]
    fn reload_keeps_committed_hold() {
        let mut handler = handler();
        commit(&mut handler);
        let config = sledge_config::parse_str("").expect("empty config");
        handler.matching.lock().replace(&config);
        assert_eq!(handler.propose(scope(2), &event(1, 1)), Decision::Swallow);
    }
}
