//! Installed, compile-catalogued Hyprland session. Candidate is not host support.
use super::{MatchingState, hyprland_research::Handler, source_queue::SourceQueue};
use crate::cli::RunArgs;
use anyhow::{Context, Result};
use parking_lot::Mutex;
use sledge_linux::hyprland::{
    EventHandler,
    catalogue::{AdmittedMetadata, Catalogue},
    listener::Listener,
    session::{InstalledDescriptor, Session},
    transport::monotonic_ns,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub(super) fn validate_rules(config: &sledge_config::Config) -> Result<()> {
    anyhow::ensure!(
        config.rules.iter().all(
            |rule| matches!(rule.trigger, sledge_core::Trigger::Hotkey(_))
                && matches!(rule.action, sledge_core::Action::SendKey { .. })
        ),
        "Hyprland candidate supports only fresh-Down send_key hotkeys; taps and source actions are not admitted"
    );
    Ok(())
}

pub(super) fn prepare(args: &RunArgs) -> Result<Option<AdmittedMetadata>> {
    let Some(path) = &args.linux_hyprland_registration else {
        return Ok(None);
    };
    let pid = args
        .linux_hyprland_pid
        .context("installed Hyprland PID required")?;
    let metadata = InstalledDescriptor::verify(path, pid)
        .context("verifying installed Hyprland process and artifacts")?;
    Catalogue::qualify(metadata)
        .context("Hyprland artifact is not admitted by the compiled catalogue")
        .map(Some)
}

pub(super) struct InstalledListener {
    listener: Listener,
    metadata: AdmittedMetadata,
    handler: Handler,
}
impl InstalledListener {
    pub(super) fn qualification(&self) -> sledge_linux::hyprland::session::Qualification {
        self.metadata.qualification()
    }

    pub(super) fn bind(
        args: &RunArgs,
        metadata: AdmittedMetadata,
        matching: Arc<Mutex<MatchingState>>,
        sources: Option<SourceQueue>,
    ) -> Result<Self> {
        let path = args
            .linux_hyprland_socket
            .as_ref()
            .context("installed Hyprland endpoint required")?;
        Ok(Self {
            listener: Listener::bind(path)
                .context("binding installed Hyprland private endpoint")?,
            metadata,
            handler: Handler { matching, sources },
        })
    }
    pub(super) fn run(mut self, stopped: &AtomicBool) -> Result<()> {
        if self.metadata.qualification()
            == sledge_linux::hyprland::session::Qualification::Supported
        {
            self.metadata.require_qualified()?;
            tracing::info!(
                feature_mask = self.metadata.feature_mask(),
                "independently accepted installed Hyprland registration; awaiting native proof"
            );
        } else {
            tracing::warn!(qualification = ?self.metadata.qualification(),
            feature_mask = self.metadata.feature_mask(),
            "isolated Hyprland qualification candidate; NOT supported host capture");
        }
        let mut session = Session::accept_admitted(
            &self.listener,
            self.metadata,
            monotonic_ns()? + 60_000_000_000,
        )
        .context("admitting installed Hyprland session")?;
        bind_matcher(&mut session, &self.handler)?;
        let result = run_session(&self.listener, &mut session, &mut self.handler, stopped);
        self.handler.disconnected();
        result
    }
}
fn bind_matcher(session: &mut Session, handler: &Handler) -> Result<()> {
    let mut state = handler.matching.lock();
    let MatchingState {
        matcher, hyprland, ..
    } = &mut *state;
    anyhow::ensure!(
        session.bind_matcher(hyprland, matcher),
        "Hyprland nonce/matcher binding rejected"
    );
    state.hyprland_health = Some(session.health());
    Ok(())
}
fn run_session(
    listener: &Listener,
    session: &mut Session,
    handler: &mut Handler,
    stopped: &AtomicBool,
) -> Result<()> {
    while !stopped.load(Ordering::Acquire) {
        let step = session.step_if_ready(handler, monotonic_ns()? + 100_000_000);
        let (needs_reconciliation, recovery) = {
            let mut state = handler.matching.lock();
            state.hyprland_health = Some(session.health());
            (
                state.hyprland.needs_reconciliation(),
                state.hyprland.recovery_state(),
            )
        };
        if step.is_err() || needs_reconciliation {
            tracing::warn!(needs_reconciliation, ?recovery, error = ?step.err(),
                "installed Hyprland settlement uncertain; fresh native reconciliation required");
            session.request_reconciliation(handler);
            reconnect(listener, session, handler, stopped)?;
        }
    }
    Ok(())
}
fn reconnect(
    listener: &Listener,
    session: &mut Session,
    handler: &Handler,
    stopped: &AtomicBool,
) -> Result<()> {
    while !stopped.load(Ordering::Acquire) {
        match session.reconnect_for(listener, std::time::Duration::from_millis(100)) {
            Ok(()) => return bind_matcher(session, handler),
            Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {}
            Err(error) => return Err(error).context("installed Hyprland reconnect admission"),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(trigger: &str, action: &str) -> sledge_config::Config {
        sledge_config::parse_str(&format!(
            "[[binding]]\ntrigger = {trigger}\naction = {action}\n"
        ))
        .expect("valid fixture config")
    }

    #[test]
    fn candidate_rules_reject_unadmitted_features() {
        let hotkey = "{ key = \"KeyA\", mods = [] }";
        let send = "{ type = \"send_key\", key = \"KeyB\", mods = [] }";
        assert!(validate_rules(&config(hotkey, send)).is_ok());
        assert!(
            validate_rules(&config(
                hotkey,
                "{ type = \"set_input_source\", id = \"fixture\" }"
            ))
            .is_err()
        );
        assert!(
            validate_rules(&config(
                "{ tap = \"LeftAlt\", count = 2, within_ms = 300 }",
                send
            ))
            .is_err()
        );
    }
}
