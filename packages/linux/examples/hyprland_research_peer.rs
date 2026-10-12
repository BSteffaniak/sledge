//! Isolated guest fixture; no capture qualification, discovery, or config parsing.
#[cfg(target_os = "linux")]
mod linux {
    use sledge_core::trigger::{HotkeyTrigger, Trigger};
    use sledge_core::{Action, KeyCode, Matcher, Modifiers, Rule, RuleSet};
    use sledge_linux::desktop::FocusCategory;
    use sledge_linux::hyprland::listener::Listener;
    use sledge_linux::hyprland::matcher::MatcherAdapter;
    use sledge_linux::hyprland::transport::{Peer, monotonic_ns};
    use sledge_linux::hyprland::wire::{Body, Decision, Event, Identity, Scope};
    use sledge_linux::hyprland::{Bridge, DeliveryReport, EventHandler, ResearchFault};
    use std::{
        io,
        path::PathBuf,
        time::{Duration, Instant},
    };

    struct Options {
        endpoint: PathBuf,
        pid: i32,
        identity: Identity,
        packets: u32,
        reconnects: u32,
        ms: u64,
        fault: ResearchFault,
        index: u64,
        category: Option<FocusCategory>,
        app: Option<String>,
        mods: Modifiers,
    }
    fn invalid() -> io::Error {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: hyprland_research_peer ENDPOINT PID HASH ARTIFACT MAX_PACKETS TOTAL_MS [normal|delay-decision|drop-decision|disconnect-before-decision|disconnect-after-decision] [PROPOSAL_INDEX] [DELAY_MS] [any|wayland|x11] [EXACT_APP] [none|shift] [RECONNECTS(0..4)] (isolated A->B fixture)",
        )
    }
    fn options() -> io::Result<Options> {
        let args = std::env::args().skip(1).collect::<Vec<_>>();
        if !(6..=13).contains(&args.len()) {
            return Err(invalid());
        }
        let pid = args[1].parse::<i32>().map_err(|_| invalid())?;
        let packets = args[4].parse::<u32>().map_err(|_| invalid())?;
        let ms = args[5].parse::<u64>().map_err(|_| invalid())?;
        let index = args
            .get(7)
            .map_or(Ok(1), |s| s.parse::<u64>())
            .map_err(|_| invalid())?;
        let delay = args
            .get(8)
            .map_or(Ok(20), |s| s.parse::<u64>())
            .map_err(|_| invalid())?;
        if pid <= 0
            || !(1..=65536).contains(&packets)
            || !(1..=60000).contains(&ms)
            || index == 0
            || delay > 1000
        {
            return Err(invalid());
        }
        let fault = match args.get(6).map_or("normal", String::as_str) {
            "normal" => ResearchFault::None,
            "delay-decision" => ResearchFault::DelayDecision(Duration::from_millis(delay)),
            "drop-decision" => ResearchFault::DropDecision,
            "disconnect-before-decision" => ResearchFault::DisconnectBeforeDecision,
            "disconnect-after-decision" => ResearchFault::DisconnectAfterDecision,
            _ => return Err(invalid()),
        };
        let category = match args.get(9).map_or("any", String::as_str) {
            "any" => None,
            "wayland" => Some(FocusCategory::Wayland),
            "x11" => Some(FocusCategory::X11Class),
            _ => return Err(invalid()),
        };
        let app = fixture_app(category.as_ref(), args.get(10).map(String::as_str))?;
        let reconnects = args
            .get(12)
            .map_or(Ok(0), |s| s.parse::<u32>())
            .map_err(|_| invalid())?;
        if reconnects > 4 {
            return Err(invalid());
        }
        let mods = match args.get(11).map_or("none", String::as_str) {
            "none" => Modifiers::empty(),
            "shift" => Modifiers::SHIFT,
            _ => return Err(invalid()),
        };
        Ok(Options {
            endpoint: PathBuf::from(&args[0]),
            pid,
            identity: Identity {
                capabilities: 0,
                api_hash: args[2].clone(),
                artifact: args[3].clone(),
            },
            packets,
            reconnects,
            ms,
            fault,
            index,
            category,
            app,
            mods,
        })
    }
    fn fixture_app(
        category: Option<&FocusCategory>,
        argument: Option<&str>,
    ) -> io::Result<Option<String>> {
        // Empty positional placeholder is necessary to select later fixture
        // arguments, but must not create an impossible app-scoped rule.
        let app = argument.filter(|value| !value.is_empty());
        if category.is_some() && app.is_none() {
            return Err(invalid());
        }
        Ok(app.map(str::to_owned))
    }
    struct Handler {
        adapter: MatcherAdapter,
        proposals: u64,
        connection: u32,
        fault_used: bool,
        fault: ResearchFault,
        index: u64,
        category: Option<FocusCategory>,
        app: Option<String>,
        evidence: Vec<String>,
    }
    impl EventHandler for Handler {
        fn propose(&mut self, scope: Scope, event: &Event) -> Decision {
            self.proposals += 1;
            let focus = MatcherAdapter::focus(event);
            let app = focus
                .as_ref()
                .filter(|f| {
                    self.category.as_ref().is_none_or(|c| *c == f.category)
                        && self.app.as_ref().is_none_or(|a| *a == f.value)
                })
                .map(|f| f.value.as_str());
            self.adapter
                .propose(scope, event, app, Instant::now())
                .unwrap_or(Decision::Pass)
        }
        fn research_fault(&mut self, scope: Scope, _: &Event) -> ResearchFault {
            if self.fault_used || self.proposals != self.index {
                return ResearchFault::None;
            }
            self.fault_used = true;
            self.evidence.push(format!(
                "fault connection={} nonce={} generation={} epoch={} sequence={} deadline={} selected={:?}",
                self.connection,
                scope.nonce,
                scope.generation,
                scope.lock_epoch,
                scope.sequence,
                scope.deadline_ns,
                self.fault
            ));
            self.fault
        }
        fn delivery(
            &mut self,
            scope: Scope,
            event: &Event,
            decision: Decision,
            report: DeliveryReport,
        ) {
            if event.state != 1 {
                self.adapter.settle_device_event(scope, event, &report);
            } else {
                self.adapter.on_delivery(scope, &report);
            }
            self.evidence.push(format!("outcome connection={} nonce={} generation={} epoch={} sequence={} deadline={} key={} state={} focus_epoch={} focus_kind={} focus={:?} decision={decision:?} report={report:?}", self.connection, scope.nonce, scope.generation, scope.lock_epoch, scope.sequence, scope.deadline_ns, event.key, event.state, event.focus_epoch, event.focus_kind, event.focus));
        }
        fn reset(&mut self, scope: Scope, body: &Body) {
            if matches!(body, Body::Reset(8)) {
                let accepted = self.adapter.reconciled(scope, body);
                self.evidence.push(format!(
                    "reconciliation connection={} nonce={} scope={scope:?} accepted={accepted}",
                    self.connection, scope.nonce
                ));
            } else if matches!(body, Body::Reset(3)) && scope.generation == 0 {
                let armed = self.adapter.await_lifecycle_reconciliation(scope, body);
                if !armed {
                    self.adapter.invalidate_speculation();
                }
                self.evidence.push(format!(
                    "lifecycle connection={} scope={scope:?} armed={armed}",
                    self.connection
                ));
            } else if matches!(body, Body::Retire(_)) {
                self.adapter.retire_device(scope.generation);
            } else {
                self.adapter.invalidate_speculation();
            }
            self.evidence.push(format!(
                "control connection={} scope={scope:?} body={body:?}",
                self.connection
            ));
        }
        fn disconnected(&mut self) {
            self.adapter.disconnected();
        }
    }
    fn handler(options: &Options) -> Handler {
        let matcher = Matcher::new(RuleSet::new(vec![Rule {
            trigger: Trigger::Hotkey(HotkeyTrigger {
                key: KeyCode::KeyA,
                mods: options.mods,
            }),
            action: Action::SendKey {
                key: KeyCode::KeyB,
                mods: Modifiers::empty(),
            },
            when_app_in: options.app.clone().map(|a| vec![a]),
        }]));
        Handler {
            adapter: MatcherAdapter::new(matcher),
            proposals: 0,
            connection: 0,
            fault_used: false,
            fault: options.fault,
            index: options.index,
            category: options.category.clone(),
            app: options.app.clone(),
            evidence: Vec::with_capacity(usize::try_from(options.packets).unwrap_or(65536) + 1),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn normal8_empty_placeholder_boundary_c_release_then_fresh_a_replaces() {
            let app = fixture_app(None, Some("")).unwrap();
            assert!(app.is_none());
            assert!(fixture_app(Some(&FocusCategory::Wayland), Some("")).is_err());
            assert!(fixture_app(Some(&FocusCategory::X11Class), None).is_err());
            assert_eq!(
                fixture_app(Some(&FocusCategory::Wayland), Some("wev"))
                    .unwrap()
                    .as_deref(),
                Some("wev")
            );
            let options = Options {
                endpoint: PathBuf::new(),
                pid: 1,
                identity: Identity {
                    capabilities: 0,
                    api_hash: String::new(),
                    artifact: String::new(),
                },
                packets: 64,
                reconnects: 0,
                ms: 30000,
                fault: ResearchFault::None,
                index: 1,
                category: None,
                app,
                mods: Modifiers::empty(),
            };
            let mut handler = handler(&options);
            let mut scope = Scope {
                generation: 0,
                lock_epoch: 7,
                sequence: 5,
                deadline_ns: 100,
                nonce: 5620230241071262401,
            };
            assert!(handler.adapter.await_reconciliation(scope.nonce));
            handler.reset(scope, &Body::Reset(8));
            scope.generation = 2;
            scope.sequence = 6;
            let mut event = Event {
                key: 46,
                state: 1,
                time_ms: 0,
                modifiers: 0,
                focus_epoch: 6,
                focus_kind: 1,
                focus: "wev".into(),
            };
            assert_eq!(handler.propose(scope, &event), Decision::Pass);
            handler.delivery(
                scope,
                &event,
                Decision::Pass,
                DeliveryReport::Acknowledged(sledge_linux::hyprland::wire::Outcome {
                    result: sledge_linux::hyprland::wire::ResultKind::OriginalAdmitted,
                    reason: 0,
                    key: 46,
                    state: 1,
                    focus_epoch: 6,
                }),
            );
            scope.sequence = 7;
            event.state = 0;
            handler.delivery(
                scope,
                &event,
                Decision::Pass,
                DeliveryReport::Acknowledged(sledge_linux::hyprland::wire::Outcome {
                    result: sledge_linux::hyprland::wire::ResultKind::OriginalAdmitted,
                    reason: 0,
                    key: 46,
                    state: 0,
                    focus_epoch: 6,
                }),
            );
            scope.sequence = 8;
            event.key = 30;
            event.state = 1;
            assert_eq!(
                handler.propose(scope, &event),
                Decision::Replace {
                    key: 48,
                    modifiers: 0
                }
            );
        }
        #[test]
        fn typed_exact_app_and_shift_fixture_do_not_cross_protocol_or_modifier_scope() {
            for (category, modifiers, expected) in [
                (FocusCategory::Wayland, 1, true),
                (FocusCategory::X11Class, 1, false),
                (FocusCategory::Wayland, 0, false),
            ] {
                let options = Options {
                    endpoint: PathBuf::new(),
                    pid: 1,
                    identity: Identity {
                        capabilities: 0,
                        api_hash: String::new(),
                        artifact: String::new(),
                    },
                    packets: 10,
                    reconnects: 0,
                    ms: 100,
                    fault: ResearchFault::None,
                    index: 1,
                    category: Some(FocusCategory::Wayland),
                    app: Some("app".into()),
                    mods: Modifiers::SHIFT,
                };
                let mut handler = handler(&options);
                let scope = Scope {
                    generation: 1,
                    lock_epoch: 1,
                    sequence: 1,
                    deadline_ns: 100,
                    nonce: 7,
                };
                let event = Event {
                    key: 30,
                    state: 1,
                    time_ms: 0,
                    modifiers,
                    focus_epoch: 1,
                    focus_kind: if category == FocusCategory::Wayland {
                        1
                    } else {
                        2
                    },
                    focus: "app".into(),
                };
                assert_eq!(
                    matches!(handler.propose(scope, &event), Decision::Replace { .. }),
                    expected
                );
            }
        }
    }
    #[cfg(test)]
    mod reconnect_tests {
        use super::*;
        use sledge_linux::hyprland::transport::Transport;
        use sledge_linux::hyprland::wire::{Outcome, Packet, ResultKind};
        use std::os::unix::fs::DirBuilderExt;
        fn scope(nonce: u64, sequence: u64, generation: u64) -> Scope {
            Scope {
                generation,
                lock_epoch: 0,
                sequence,
                deadline_ns: monotonic_ns().unwrap() + 10_000_000,
                nonce,
            }
        }
        fn event() -> Event {
            Event {
                key: 30,
                state: 1,
                time_ms: 0,
                modifiers: 0,
                focus_epoch: 1,
                focus_kind: 1,
                focus: "app".into(),
            }
        }
        fn connect(
            listener: &Listener,
            path: &PathBuf,
            identity: &Identity,
            nonce: u64,
        ) -> (Transport, Transport) {
            let deadline = monotonic_ns().unwrap() + 100_000_000;
            let peer = Peer {
                uid: nix::unistd::geteuid().as_raw(),
                pid: i32::try_from(std::process::id()).unwrap(),
            };
            let native = Transport::connect(path, peer, deadline).unwrap();
            let rust = listener.accept(peer, deadline).unwrap();
            native
                .send(
                    &Packet {
                        scope: Scope {
                            generation: 0,
                            lock_epoch: 0,
                            sequence: 0,
                            deadline_ns: 0,
                            nonce,
                        },
                        body: Body::Hello(identity.clone()),
                    },
                    deadline,
                )
                .unwrap();
            (native, rust)
        }
        #[test]
        fn dropped_outcome_reconnect_same_listener_matcher_requires_reason8() {
            let directory = std::env::temp_dir().join(format!(
                "sledge-peer-reconnect-{}-{}",
                std::process::id(),
                monotonic_ns().unwrap()
            ));
            std::fs::DirBuilder::new()
                .mode(0o700)
                .create(&directory)
                .unwrap();
            let path = directory.join("peer");
            let listener = Listener::bind(&path).unwrap();
            let options = Options {
                endpoint: path.clone(),
                pid: 1,
                identity: Identity {
                    capabilities: 0,
                    api_hash: "api".into(),
                    artifact: "artifact".into(),
                },
                packets: 20,
                reconnects: 1,
                ms: 1000,
                fault: ResearchFault::DisconnectAfterDecision,
                index: 1,
                category: None,
                app: None,
                mods: Modifiers::empty(),
            };
            let (native, rust) = connect(&listener, &path, &options.identity, 7);
            let mut bridge = Bridge::passive_handshake(
                rust,
                &options.identity,
                monotonic_ns().unwrap() + 100_000_000,
            )
            .unwrap();
            native
                .receive(monotonic_ns().unwrap() + 100_000_000)
                .unwrap();
            let mut handler = handler(&options);
            handler
                .adapter
                .await_reconciliation(bridge.connection_nonce());
            let reset = Packet {
                scope: scope(7, 1, 0),
                body: Body::Reset(8),
            };
            native.send(&reset, reset.scope.deadline_ns).unwrap();
            bridge
                .research_step(&mut handler, reset.scope.deadline_ns)
                .unwrap();
            // Genuine native dropped Outcome: decision commits, ACK is absent.
            handler.fault = ResearchFault::None;
            let down = Packet {
                scope: scope(7, 2, 1),
                body: Body::Event(event()),
            };
            native.send(&down, down.scope.deadline_ns).unwrap();
            assert!(
                bridge
                    .research_step(&mut handler, down.scope.deadline_ns)
                    .is_err()
            );
            drop(native);
            let (native, rust) = connect(&listener, &path, &options.identity, 8);
            bridge
                .reconnect(
                    rust,
                    &options.identity,
                    monotonic_ns().unwrap() + 100_000_000,
                )
                .unwrap();
            native
                .receive(monotonic_ns().unwrap() + 100_000_000)
                .unwrap();
            handler.connection = 1;
            assert!(
                handler
                    .adapter
                    .await_reconciliation(bridge.connection_nonce())
            );
            assert!(
                handler
                    .adapter
                    .propose(scope(8, 1, 1), &event(), None, Instant::now())
                    .is_err()
            );
            let reset = Packet {
                scope: scope(8, 1, 0),
                body: Body::Reset(8),
            };
            native.send(&reset, reset.scope.deadline_ns).unwrap();
            bridge
                .research_step(&mut handler, reset.scope.deadline_ns)
                .unwrap();
            let down = Packet {
                scope: scope(8, 2, 1),
                body: Body::Event(event()),
            };
            native.send(&down, down.scope.deadline_ns).unwrap();
            native
                .send(
                    &Packet {
                        scope: down.scope,
                        body: Body::Outcome(Outcome {
                            result: ResultKind::Replaced,
                            reason: 0,
                            key: 30,
                            state: 1,
                            focus_epoch: 1,
                        }),
                    },
                    down.scope.deadline_ns,
                )
                .unwrap();
            bridge
                .research_step(&mut handler, down.scope.deadline_ns)
                .unwrap();
            assert_eq!(
                native.receive(down.scope.deadline_ns).unwrap().body,
                Body::Decision(Decision::Replace {
                    key: 48,
                    modifiers: 0
                })
            );
            assert!(
                handler
                    .evidence
                    .iter()
                    .any(|line| line.contains("connection=1") && line.contains("accepted=true"))
            );
            // A selected fault is one-shot even when no later proposal occurs.
            handler.proposals = 1;
            handler.fault_used = false;
            handler.fault = ResearchFault::DisconnectAfterDecision;
            assert!(matches!(
                handler.research_fault(down.scope, &event()),
                ResearchFault::DisconnectAfterDecision
            ));
            assert!(matches!(
                handler.research_fault(down.scope, &event()),
                ResearchFault::None
            ));
            drop(bridge);
            drop(native);
            drop(listener);
            std::fs::remove_dir(directory).unwrap();
        }
    }
    pub fn run() -> io::Result<()> {
        let options = options()?;
        let listener = Listener::bind(&options.endpoint)?;
        let deadline = monotonic_ns()? + options.ms * 1_000_000;
        println!("research listening capabilities=0 qualified=false fixture=A-to-B");
        let transport = listener.accept(
            Peer {
                uid: nix::unistd::geteuid().as_raw(),
                pid: options.pid,
            },
            deadline,
        )?;
        let mut bridge = Bridge::passive_handshake(
            transport,
            &options.identity,
            deadline.min(monotonic_ns()? + 10_000_000),
        )?;
        let mut handler = handler(&options);
        handler
            .adapter
            .await_reconciliation(bridge.connection_nonce());
        handler.evidence.push(format!(
            "connection index=0 nonce={} awaiting_reason8=true",
            bridge.connection_nonce()
        ));
        let mut result = Ok(());
        for _ in 0..options.packets {
            if let Err(error) = bridge.research_step(&mut handler, deadline) {
                handler.evidence.push(format!(
                    "disconnect connection={} nonce={} error_kind={:?} error={} no_replay=true",
                    handler.connection,
                    bridge.connection_nonce(),
                    error.kind(),
                    error
                ));
                if handler.connection >= options.reconnects {
                    result = Err(error);
                    break;
                }
                let admission = (|| {
                    let transport = listener.accept(
                        Peer {
                            uid: nix::unistd::geteuid().as_raw(),
                            pid: options.pid,
                        },
                        deadline,
                    )?;
                    bridge.reconnect(
                        transport,
                        &options.identity,
                        deadline.min(monotonic_ns()? + 10_000_000),
                    )?;
                    if !handler
                        .adapter
                        .await_reconciliation(bridge.connection_nonce())
                    {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "recovery nonce binding rejected",
                        ));
                    }
                    handler.connection += 1;
                    handler.evidence.push(format!(
                        "connection index={} nonce={} awaiting_reason8=true",
                        handler.connection,
                        bridge.connection_nonce()
                    ));
                    Ok(())
                })();
                if let Err(error) = admission {
                    result = Err(error);
                    break;
                }
            }
        }
        // Buffered worker evidence only after bounded transactions; no callback I/O.
        for line in handler.evidence {
            println!("{line}");
        }
        println!(
            "research proposals={} qualified=false status={}",
            handler.proposals,
            if result.is_ok() {
                "packet_limit"
            } else {
                "failed_bounded_no_replay"
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
