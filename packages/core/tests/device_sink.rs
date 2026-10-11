//! Backwards compatibility and trait-object routing for stream-aware sinks.
use std::time::Instant;

use sledge_core::{
    Action, BackendVerdict, DeviceId, EventKind, EventSink, HotkeyTrigger, KeyCode, KeyEvent,
    Matcher, Modifiers, Rule, RuleSet, Trigger, Verdict,
};

fn key(kind: EventKind) -> KeyEvent {
    KeyEvent {
        code: KeyCode::KeyA,
        kind,
        mods: Modifiers::empty(),
    }
}

struct LegacySink;

impl EventSink for LegacySink {
    fn on_event(&mut self, event: KeyEvent, focused_app: Option<&str>) -> BackendVerdict {
        assert_eq!(event, key(EventKind::KeyDown));
        assert_eq!(focused_app, Some("app"));
        BackendVerdict::Swallow
    }
}

#[test]
fn existing_sink_implements_only_on_event_and_device_defaults_forward() {
    let mut sink: Box<dyn EventSink> = Box::new(LegacySink);
    assert!(matches!(
        sink.on_device_event(DeviceId(9), key(EventKind::KeyDown), Some("app")),
        BackendVerdict::Swallow
    ));
    sink.on_device_reset(DeviceId(9));
    sink.on_stream_reset();
}

struct DeviceSink(Matcher);

impl DeviceSink {
    fn verdict(verdict: Verdict) -> BackendVerdict {
        match verdict {
            Verdict::Pass => BackendVerdict::Pass,
            Verdict::Swallow | Verdict::Replace(_) => BackendVerdict::Swallow,
        }
    }
}

impl EventSink for DeviceSink {
    fn on_event(&mut self, event: KeyEvent, focused_app: Option<&str>) -> BackendVerdict {
        Self::verdict(self.0.dispatch(event, focused_app, Instant::now()))
    }

    fn on_device_event(
        &mut self,
        device: DeviceId,
        event: KeyEvent,
        focused_app: Option<&str>,
    ) -> BackendVerdict {
        Self::verdict(
            self.0
                .dispatch_device(device, event, focused_app, Instant::now()),
        )
    }

    fn on_device_reset(&mut self, device: DeviceId) {
        self.0.reset_device(device);
    }

    fn on_stream_reset(&mut self) {
        self.0.reset();
    }
}

#[test]
fn trait_object_preserves_identity_and_routes_device_and_global_resets() {
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
    let mut sink: Box<dyn EventSink> = Box::new(DeviceSink(matcher));
    for id in [DeviceId(1), DeviceId(2)] {
        assert!(matches!(
            sink.on_device_event(id, key(EventKind::KeyDown), None),
            BackendVerdict::Swallow
        ));
    }
    assert!(matches!(
        sink.on_device_event(DeviceId(3), key(EventKind::KeyUp), None),
        BackendVerdict::Pass
    ));
    sink.on_device_reset(DeviceId(1));
    assert!(matches!(
        sink.on_device_event(DeviceId(1), key(EventKind::KeyUp), None),
        BackendVerdict::Pass
    ));
    assert!(matches!(
        sink.on_device_event(DeviceId(2), key(EventKind::KeyUp), None),
        BackendVerdict::Swallow
    ));
    assert!(matches!(
        sink.on_device_event(DeviceId(2), key(EventKind::KeyDown), None),
        BackendVerdict::Swallow
    ));
    sink.on_stream_reset();
    assert!(matches!(
        sink.on_device_event(DeviceId(2), key(EventKind::KeyUp), None),
        BackendVerdict::Pass
    ));
    // macOS/legacy entrypoint still uses the default stream.
    assert!(matches!(
        sink.on_event(key(EventKind::KeyDown), None),
        BackendVerdict::Swallow
    ));
    assert!(matches!(
        sink.on_event(key(EventKind::KeyUp), None),
        BackendVerdict::Swallow
    ));
}
