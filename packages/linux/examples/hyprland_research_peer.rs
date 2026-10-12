//! Explicit isolated research fixture, never a qualified production backend.
#[cfg(target_os = "linux")]
mod linux {
    use sledge_core::trigger::{HotkeyTrigger, Trigger};
    use sledge_core::{Action, KeyCode, Matcher, Modifiers, Rule, RuleSet};
    use sledge_linux::hyprland::listener::Listener;
    use sledge_linux::hyprland::matcher::MatcherAdapter;
    use sledge_linux::hyprland::transport::{Peer, monotonic_ns};
    use sledge_linux::hyprland::wire::{Body, Decision, Event, Identity, Scope};
    use sledge_linux::hyprland::{Bridge, DeliveryReport, EventHandler};
    use std::{io, path::Path, time::Instant};
    struct Handler {
        adapter: MatcherAdapter,
        proposals: u64,
        acknowledgments: u64,
        uncertain: u64,
    }
    impl EventHandler for Handler {
        fn propose(&mut self, scope: Scope, event: &Event) -> Decision {
            self.proposals += 1;
            let focus = MatcherAdapter::focus(event);
            self.adapter
                .propose(
                    scope,
                    event,
                    focus.as_ref().map(|f| f.value.as_str()),
                    Instant::now(),
                )
                .unwrap_or(Decision::Pass)
        }
        fn delivery(&mut self, scope: Scope, event: &Event, _: Decision, report: DeliveryReport) {
            if matches!(report, DeliveryReport::Uncertain) {
                self.uncertain += 1;
            } else {
                self.acknowledgments += 1;
            }
            if event.state != 1 {
                self.adapter.settle_device_event(scope, event, &report);
            } else {
                self.adapter.on_delivery(scope, &report);
            }
        }
        fn reset(&mut self, _: Scope, _: &Body) {
            self.adapter.invalidate_speculation();
        }
        fn disconnected(&mut self) {
            self.adapter.disconnected();
        }
    }
    fn invalid() -> io::Error {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: hyprland_research_peer ABS_ENDPOINT PID API_HASH ARTIFACT MAX_PACKETS TOTAL_MS (isolated research only; fixed A->B)",
        )
    }
    pub fn run() -> io::Result<()> {
        let args = std::env::args().skip(1).collect::<Vec<_>>();
        if args.len() != 6 {
            return Err(invalid());
        }
        let pid = args[1].parse::<i32>().map_err(|_| invalid())?;
        let packets = args[4].parse::<u32>().map_err(|_| invalid())?;
        let ms = args[5].parse::<u64>().map_err(|_| invalid())?;
        if pid <= 0 || !(1..=65536).contains(&packets) || !(1..=60000).contains(&ms) {
            return Err(invalid());
        }
        let listener = Listener::bind(Path::new(&args[0]))?;
        let deadline = monotonic_ns()? + ms * 1_000_000;
        println!("research listening capabilities=0 qualified=false fixture=A-to-B");
        let transport = listener.accept(
            Peer {
                uid: nix::unistd::geteuid().as_raw(),
                pid,
            },
            deadline,
        )?;
        let identity = Identity {
            capabilities: 0,
            api_hash: args[2].clone(),
            artifact: args[3].clone(),
        };
        let mut bridge = Bridge::passive_handshake(
            transport,
            &identity,
            deadline.min(monotonic_ns()? + 10_000_000),
        )?;
        let matcher = Matcher::new(RuleSet::new(vec![Rule {
            trigger: Trigger::Hotkey(HotkeyTrigger {
                key: KeyCode::KeyA,
                mods: Modifiers::empty(),
            }),
            action: Action::SendKey {
                key: KeyCode::KeyB,
                mods: Modifiers::empty(),
            },
            when_app_in: None,
        }]));
        let mut handler = Handler {
            adapter: MatcherAdapter::new(matcher),
            proposals: 0,
            acknowledgments: 0,
            uncertain: 0,
        };
        let mut result = Ok(());
        for _ in 0..packets {
            if let Err(error) = bridge.research_step(&mut handler, deadline) {
                result = Err(error);
                break;
            }
        }
        println!(
            "research proposals={} acknowledgments={} uncertain={} qualified=false status={}",
            handler.proposals,
            handler.acknowledgments,
            handler.uncertain,
            if result.is_ok() {
                "packet_limit"
            } else {
                "failed_no_retry"
            }
        );
        result
    }
}
#[cfg(target_os = "linux")]
fn main() -> std::process::ExitCode {
    match linux::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("research failed: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}
#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("Linux isolated research only");
}
