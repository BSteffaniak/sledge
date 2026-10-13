//! Installed metadata verification and daemon-facing same-user session.
//! No production qualification catalogue is installed: identity is not authority.
use super::listener::Listener;
use super::matcher::MatcherTransactions;
use super::transport::{Peer, monotonic_ns};
use super::wire::Identity;
use super::{Bridge, EventHandler};
use serde::Deserialize;
use sledge_core::Matcher;
use std::os::unix::fs::MetadataExt;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstalledDescriptor {
    pub wire_version: u16,
    pub qualification_revision: u64,
    pub build_id: String,
    pub compositor_path: PathBuf,
    pub compositor_sha256: String,
    pub sdk_api_hash: String,
    pub plugin_path: PathBuf,
    pub plugin_sha256: String,
    pub capabilities: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Qualification {
    Unqualified,
    Candidate,
    Supported,
}
/// Constructible only by local file/process verification; fields are not grants.
pub struct VerifiedMetadata {
    descriptor: InstalledDescriptor,
    peer: Peer,
    start_time: u64,
    pub(super) descriptor_digest: String,
}
fn denied(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}
impl InstalledDescriptor {
    /// Verify bounded immutable installed metadata and actual process/files off
    /// callback. PID/starttime/UID/executable/mapping must remain stable. Plugin
    /// must already be mapped in the specified compositor; no load/discovery.
    /// # Errors
    /// Rejects mutable paths, forged digests, wrong process or missing mappings.
    pub fn verify(registration_path: &Path, pid: i32) -> io::Result<VerifiedMetadata> {
        let path = immutable_file(registration_path)?;
        let bytes = bounded_read(&path, 16 * 1024)?;
        let descriptor: Self = serde_json::from_slice(&bytes)
            .map_err(|_| denied("invalid installed descriptor schema"))?;
        descriptor.validate()?;
        let peer = Peer {
            uid: nix::unistd::geteuid().as_raw(),
            pid,
        };
        let before = process_start(peer)?;
        verify_files_and_process(&descriptor, peer)?;
        if process_start(peer)? != before {
            return Err(denied(
                "compositor PID identity changed during verification",
            ));
        }
        Ok(VerifiedMetadata {
            descriptor,
            peer,
            start_time: before,
            descriptor_digest: format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(&bytes)),
        })
    }
    fn validate(&self) -> io::Result<()> {
        if self.wire_version != 1
            || self.capabilities & !15 != 0
            || !self
                .build_id
                .strip_prefix("sledge-stock-v1:")
                .is_some_and(digest_syntax)
            || self.sdk_api_hash.is_empty()
            || self.sdk_api_hash.len() > 192
            || self.sdk_api_hash.contains('\0')
            || !digest_syntax(&self.compositor_sha256)
            || !digest_syntax(&self.plugin_sha256)
        {
            return Err(denied("invalid descriptor version/build/hash/capabilities"));
        }
        Ok(())
    }
}
impl VerifiedMetadata {
    #[must_use]
    pub const fn qualification(&self) -> Qualification {
        Qualification::Unqualified
    }
    #[must_use]
    pub const fn descriptor(&self) -> &InstalledDescriptor {
        &self.descriptor
    }
    /// Fail closed until a compiled/immutable independently accepted catalogue
    /// binds this entire descriptor to actual supported behavior. Neither a
    /// matching SDK nor caller JSON capabilities can create qualification.
    /// # Errors
    /// Currently always Unqualified: no accepted production registration exists.
    pub fn require_qualified(&self) -> io::Result<()> {
        Err(denied(
            "Hyprland build metadata verified but production unqualified",
        ))
    }
    fn revalidate(&self) -> io::Result<()> {
        if process_start(self.peer)? != self.start_time {
            return Err(denied("compositor PID/starttime changed"));
        }
        verify_files_and_process(&self.descriptor, self.peer)?;
        if process_start(self.peer)? != self.start_time {
            return Err(denied("compositor PID/starttime changed"));
        }
        Ok(())
    }
    fn passive_identity(&self) -> Identity {
        Identity {
            capabilities: 0,
            api_hash: self.descriptor.sdk_api_hash.clone(),
            artifact: self.descriptor.build_id.clone(),
        }
    }
}
fn digest_syntax(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn immutable_file(path: &Path) -> io::Result<PathBuf> {
    let canonical = fs::canonicalize(path)?;
    let relative = canonical
        .strip_prefix("/nix/store")
        .map_err(|_| denied("registration/artifact is not immutable store file"))?;
    if relative.components().count() < 2 {
        return Err(denied("invalid immutable artifact path"));
    }
    let metadata = fs::metadata(&canonical)?;
    if !metadata.is_file() || metadata.uid() != 0 || metadata.mode() & 0o222 != 0 {
        return Err(denied("artifact must be immutable root-owned regular file"));
    }
    Ok(canonical)
}
fn bounded_read(path: &Path, max: usize) -> io::Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(u64::try_from(max).map_err(|_| denied("read bound overflow"))? + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > max {
        return Err(denied("verification input oversized"));
    }
    Ok(bytes)
}
fn hash_file(path: &Path) -> io::Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = fs::File::open(path)?;
    if file.metadata()?.len() > 512 * 1024 * 1024 {
        return Err(denied("artifact exceeds hashing budget"));
    }
    let mut hash = Sha256::new();
    let mut buffer = [0; 16 * 1024];
    let mut total = 0_u64;
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        total += u64::try_from(n).map_err(|_| denied("hash bound overflow"))?;
        if total > 512 * 1024 * 1024 {
            return Err(denied("artifact exceeds hashing budget"));
        }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
fn process_start(peer: Peer) -> io::Result<u64> {
    if peer.pid <= 0 || peer.uid != nix::unistd::geteuid().as_raw() {
        return Err(denied("invalid same-user compositor PID"));
    }
    let process = PathBuf::from(format!("/proc/{}", peer.pid));
    if fs::metadata(&process)?.uid() != peer.uid {
        return Err(denied("compositor UID mismatch"));
    }
    let stat = bounded_read(&process.join("stat"), 4096)?;
    let stat = std::str::from_utf8(&stat).map_err(|_| denied("invalid process stat"))?;
    let end = stat
        .rfind(')')
        .ok_or_else(|| denied("invalid process stat"))?;
    if matches!(
        stat[end + 1..].split_whitespace().next(),
        Some("Z" | "X" | "x")
    ) {
        return Err(denied("compositor process exited"));
    }
    parse_start(stat)
}
fn parse_start(stat: &str) -> io::Result<u64> {
    let end = stat
        .rfind(')')
        .ok_or_else(|| denied("invalid process stat"))?;
    stat[end + 1..]
        .split_whitespace()
        .nth(19)
        .ok_or_else(|| denied("missing process starttime"))?
        .parse()
        .map_err(|_| denied("invalid process starttime"))
}
fn parse_mapping_device(device: &str) -> Option<(u64, u64)> {
    let (major, minor) = device.split_once(':')?;
    Some((
        u64::from_str_radix(major, 16).ok()?,
        u64::from_str_radix(minor, 16).ok()?,
    ))
}
fn verify_files_and_process(d: &InstalledDescriptor, peer: Peer) -> io::Result<()> {
    let compositor = immutable_file(&d.compositor_path)?;
    let plugin = immutable_file(&d.plugin_path)?;
    if hash_file(&compositor)? != d.compositor_sha256 || hash_file(&plugin)? != d.plugin_sha256 {
        return Err(denied("installed artifact SHA256 mismatch"));
    }
    let executable = fs::canonicalize(format!("/proc/{}/exe", peer.pid))?;
    if executable != compositor || hash_file(&executable)? != d.compositor_sha256 {
        return Err(denied("compositor executable path/digest mismatch"));
    }
    let maps = bounded_read(
        Path::new(&format!("/proc/{}/maps", peer.pid)),
        4 * 1024 * 1024,
    )?;
    let maps = std::str::from_utf8(&maps).map_err(|_| denied("invalid compositor mappings"))?;
    let plugin_metadata = fs::metadata(&plugin)?;
    let plugin = plugin
        .to_str()
        .ok_or_else(|| denied("non-UTF8 plugin path"))?;
    if !maps.lines().any(|line| {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        fields.get(5).copied() == Some(plugin)
            && fields
                .get(3)
                .and_then(|device| parse_mapping_device(device))
                == Some((
                    nix::sys::stat::major(plugin_metadata.dev()),
                    nix::sys::stat::minor(plugin_metadata.dev()),
                ))
            && fields.get(4).and_then(|inode| inode.parse::<u64>().ok())
                == Some(plugin_metadata.ino())
    }) {
        return Err(denied("verified plugin not mapped in compositor"));
    }
    Ok(())
}
/// Metadata-verified passive session. Real production startup must first call
/// require_qualified; currently fails closed. Passive never invokes proposals.
/// Authenticated transport state, not capture qualification or consumer receipt.
#[derive(Clone, Copy, Debug)]
pub struct SessionHealth {
    pub connected: bool,
    pub awaiting_reconciliation: bool,
    pub locked: bool,
    pub nonce: u64,
    peer: Peer,
    start_time: u64,
}
impl SessionHealth {
    /// Cheap PID/UID/starttime check; never hashes artifacts or polls desktop.
    #[must_use]
    pub fn process_is_current(&self) -> bool {
        process_start(self.peer).is_ok_and(|start| start == self.start_time)
    }
    /// Hello alone is insufficient. Caller supplies exact matcher nonce proof.
    #[must_use]
    pub fn operational(&self, matcher_reconciled: bool) -> bool {
        self.connected
            && !self.awaiting_reconciliation
            && !self.locked
            && matcher_reconciled
            && self.process_is_current()
    }
    pub fn invalidate(&mut self) {
        self.connected = false;
        self.awaiting_reconciliation = true;
    }
}
pub struct Session {
    bridge: Bridge,
    metadata: VerifiedMetadata,
    matcher_bound: bool,
    active: bool,
    qualification: Qualification,
}
impl Session {
    /// Admit diagnostic session, not capture. Caller can inspect qualification.
    /// # Errors
    /// Reports registration/process/peer/Hello/deadline failures.
    pub fn accept(
        listener: &Listener,
        metadata: VerifiedMetadata,
        deadline: u64,
    ) -> io::Result<Self> {
        metadata.revalidate()?;
        let bridge = retry_hello(deadline, || {
            let transport = listener.accept(metadata.peer, deadline)?;
            if process_start(metadata.peer)? != metadata.start_time {
                return Err(denied("compositor PID/starttime changed at accept"));
            }
            Bridge::passive_handshake(
                transport,
                &metadata.passive_identity(),
                Self::handshake_deadline(deadline)?,
            )
        })?;
        Ok(Self {
            bridge,
            metadata,
            matcher_bound: false,
            active: false,
            qualification: Qualification::Unqualified,
        })
    }
    /// Explicit compiled-catalogue admission; Candidate is never host qualification.
    /// Same metadata/PID/UID/nonce/reconciliation path as supported session.
    /// # Errors
    /// Rejects metadata/Hello/peer failures; proposal feature mask must be 1.
    pub fn accept_admitted(
        listener: &Listener,
        admitted: super::catalogue::AdmittedMetadata,
        deadline: u64,
    ) -> io::Result<Self> {
        if admitted.feature_mask != super::catalogue::FRESH_DOWN_SEND_KEY {
            return Err(denied("unsupported admitted features"));
        }
        super::catalogue::Catalogue::recheck(&admitted.metadata)?;
        let qualification = admitted.qualification();
        let mut session = Self::accept(listener, admitted.metadata, deadline)?;
        session.qualification = qualification;
        session.active = true;
        Ok(session)
    }
    #[must_use]
    pub fn health(&self) -> SessionHealth {
        SessionHealth {
            connected: self.bridge.transport.is_some(),
            awaiting_reconciliation: self.bridge.awaiting_reconciliation,
            locked: self.bridge.locked,
            nonce: self.connection_nonce(),
            peer: self.metadata.peer,
            start_time: self.metadata.start_time,
        }
    }
    /// Current compiled admission status; Candidate never means host support.
    #[must_use]
    pub const fn qualification(&self) -> Qualification {
        self.qualification
    }
    #[must_use]
    pub const fn feature_mask(&self) -> u64 {
        if self.active {
            super::catalogue::FRESH_DOWN_SEND_KEY
        } else {
            0
        }
    }
    #[must_use]
    pub const fn metadata(&self) -> &VerifiedMetadata {
        &self.metadata
    }
    #[must_use]
    pub const fn connection_nonce(&self) -> u64 {
        self.bridge.connection_nonce()
    }
    pub fn bind_matcher(&mut self, t: &mut MatcherTransactions, m: &mut Matcher) -> bool {
        self.matcher_bound = t.await_reconciliation(m, self.connection_nonce());
        self.matcher_bound
    }
    /// Fresh transport, same native PID/generations and preserved matcher holds.
    /// # Errors
    /// Rejects changed process/mapping/permissions or stale nonce.
    pub fn reconnect(&mut self, listener: &Listener, deadline: u64) -> io::Result<()> {
        self.reverify()?;
        self.reconnect_verified(listener, deadline)
    }
    /// Verify off-callback before starting the socket admission budget.
    /// # Errors
    /// Reports verification, duration overflow and authenticated reconnect errors.
    pub fn reconnect_for(
        &mut self,
        listener: &Listener,
        wait: std::time::Duration,
    ) -> io::Result<()> {
        let deadline = admission_deadline_after(wait, || self.reverify())?;
        self.reconnect_verified(listener, deadline)
    }
    fn reverify(&self) -> io::Result<()> {
        if self.active {
            super::catalogue::Catalogue::recheck(&self.metadata)?;
        }
        self.metadata.revalidate()
    }
    fn reconnect_verified(&mut self, listener: &Listener, deadline: u64) -> io::Result<()> {
        retry_hello(deadline, || {
            let transport = listener.accept(self.metadata.peer, deadline)?;
            if process_start(self.metadata.peer)? != self.metadata.start_time {
                return Err(denied("compositor PID/starttime changed at reconnect"));
            }
            self.bridge.reconnect(
                transport,
                &self.metadata.passive_identity(),
                Self::handshake_deadline(deadline)?,
            )
        })?;
        self.matcher_bound = false;
        Ok(())
    }
    /// Close uncertain transport without clearing matcher/native ownership.
    /// Release the matching lock before invoking the handler.
    pub fn request_reconciliation(&mut self, handler: &mut impl EventHandler) {
        self.matcher_bound = false;
        self.bridge.request_reconciliation(handler);
    }
    /// Passive bounded readiness; zero claims never dispatch matcher proposals.
    /// # Errors
    /// Rejects unbound matcher and invalid packets; uncertainty closes transport.
    pub fn step_if_ready(&mut self, h: &mut impl EventHandler, deadline: u64) -> io::Result<bool> {
        if !self.matcher_bound {
            return Err(denied("matcher not bound to session nonce"));
        }
        if self.active && !self.bridge.awaiting_reconciliation && !self.bridge.locked {
            let mut filtered = DownOnly(h);
            self.bridge.admitted_step_if_ready(&mut filtered, deadline)
        } else {
            self.bridge.passive_step_if_ready(h, deadline)
        }
    }
    /// # Errors
    /// Reports monotonic clock failure/overflow.
    pub fn handshake_deadline(overall: u64) -> io::Result<u64> {
        Ok(overall.min(
            monotonic_ns()?
                .checked_add(10_000_000)
                .ok_or_else(super::wire::invalid)?,
        ))
    }
}
// A native peer can expire its 10ms Hello while full startup/reconnect
// verification runs off-callback. Drop that stale connection and admit the next
// authenticated retry within the SAME overall deadline. Protocol/auth errors
// never retry. No history/matcher state is reset by failed Hello attempts.
fn retry_hello<T>(deadline: u64, mut attempt: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    loop {
        if monotonic_ns()? >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Hyprland admission deadline expired",
            ));
        }
        match attempt() {
            Ok(value) => return Ok(value),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut
                        | io::ErrorKind::BrokenPipe
                        | io::ErrorKind::UnexpectedEof
                        | io::ErrorKind::ConnectionReset
                        | io::ErrorKind::ConnectionAborted
                ) => {}
            Err(error) => return Err(error),
        }
    }
}
fn admission_deadline_after(
    wait: std::time::Duration,
    verify: impl FnOnce() -> io::Result<()>,
) -> io::Result<u64> {
    verify()?;
    let nanos = u64::try_from(wait.as_nanos()).map_err(|_| super::wire::invalid())?;
    if nanos == 0 {
        return Err(super::wire::invalid());
    }
    monotonic_ns()?
        .checked_add(nanos)
        .ok_or_else(super::wire::invalid)
}
// No research_fault forwarding, source-side-effect grant, release/tap proposal.
struct DownOnly<'a, H>(&'a mut H);
impl<H: EventHandler> EventHandler for DownOnly<'_, H> {
    fn propose(
        &mut self,
        scope: super::wire::Scope,
        event: &super::wire::Event,
    ) -> super::wire::Decision {
        self.0.propose(scope, event)
    }
    fn delivery(
        &mut self,
        scope: super::wire::Scope,
        event: &super::wire::Event,
        decision: super::wire::Decision,
        report: super::DeliveryReport,
    ) {
        self.0.delivery(scope, event, decision, report);
    }
    fn reset(&mut self, scope: super::wire::Scope, body: &super::wire::Body) {
        self.0.reset(scope, body);
    }
    fn unlocked_epoch(&mut self, scope: super::wire::Scope, body: &super::wire::Body) {
        self.0.unlocked_epoch(scope, body);
    }
    fn disconnected(&mut self) {
        self.0.disconnected();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn descriptor_rejects_forged_schema_and_mutable_inputs() {
        assert!(
            InstalledDescriptor::verify(Path::new("/tmp/not-an-immutable-registration"), 1)
                .is_err()
        );
        assert!(
            serde_json::from_str::<InstalledDescriptor>("{\"trusted\":true,\"capabilities\":15}")
                .is_err()
        );
        assert!(!digest_syntax("unknown"));
        assert!(!digest_syntax(&"F".repeat(64)));
        assert!(digest_syntax(&"a".repeat(64)));
    }
    #[test]
    fn active_session_waits_reason8_and_never_forwards_research_faults() {
        use super::super::transport::Transport;
        use super::super::wire::{Body, Decision, Event, Outcome, Packet, ResultKind, Scope};
        struct Handler;
        impl EventHandler for Handler {
            fn propose(&mut self, _: Scope, _: &Event) -> Decision {
                Decision::Replace {
                    key: 48,
                    modifiers: 0,
                }
            }
            fn research_fault(&mut self, _: Scope, _: &Event) -> super::super::ResearchFault {
                panic!("production path forwarded research fault")
            }
            fn delivery(
                &mut self,
                _: Scope,
                _: &Event,
                _: Decision,
                _: super::super::DeliveryReport,
            ) {
            }
            fn reset(&mut self, _: Scope, _: &Body) {}
            fn disconnected(&mut self) {}
        }
        let (transport, fd) = super::super::tests::pair();
        let mut bridge = super::super::tests::bridge(transport);
        let peer = Transport::from_owned_fd(fd);
        bridge.awaiting_reconciliation = true;
        let metadata = VerifiedMetadata {
            descriptor: descriptor(),
            peer: Peer { uid: 0, pid: 1 },
            start_time: 1,
            descriptor_digest: "a".repeat(64),
        };
        let mut session = Session {
            bridge,
            metadata,
            matcher_bound: true,
            active: true,
            qualification: Qualification::Candidate,
        };
        let mut scope = super::super::tests::scope();
        scope.generation = 0;
        peer.send(
            &Packet {
                scope,
                body: Body::Reset(8),
            },
            scope.deadline_ns,
        )
        .unwrap();
        assert!(
            session
                .step_if_ready(&mut Handler, scope.deadline_ns)
                .unwrap()
        );
        scope.sequence += 1;
        scope.generation = 1;
        scope.deadline_ns = monotonic_ns().unwrap() + 10_000_000;
        let event = super::super::tests::event();
        peer.send(
            &Packet {
                scope,
                body: Body::Event(event),
            },
            scope.deadline_ns,
        )
        .unwrap();
        peer.send(
            &Packet {
                scope,
                body: Body::Outcome(Outcome {
                    result: ResultKind::Replaced,
                    reason: 0,
                    key: 30,
                    state: 1,
                    focus_epoch: 9,
                }),
            },
            scope.deadline_ns,
        )
        .unwrap();
        assert!(
            session
                .step_if_ready(&mut Handler, scope.deadline_ns)
                .unwrap()
        );
        assert_eq!(
            peer.receive(scope.deadline_ns).unwrap().body,
            Body::Decision(Decision::Replace {
                key: 48,
                modifiers: 0
            })
        );
    }
    #[test]
    fn health_requires_current_process_nonce_proof_and_live_unlocked_transport() {
        let peer = Peer {
            uid: nix::unistd::geteuid().as_raw(),
            pid: i32::try_from(std::process::id()).unwrap(),
        };
        let mut health = SessionHealth {
            connected: true,
            awaiting_reconciliation: true,
            locked: false,
            nonce: 7,
            peer,
            start_time: process_start(peer).unwrap(),
        };
        assert!(!health.operational(true)); // Hello is never operational.
        health.awaiting_reconciliation = false;
        assert!(!health.operational(false));
        assert!(health.operational(true));
        health.locked = true;
        assert!(!health.operational(true));
        health.locked = false;
        health.start_time += 1;
        assert!(!health.operational(true)); // PID reused/exited cannot retain health.
        health.start_time -= 1;
        health.invalidate();
        assert!(!health.operational(true));
    }
    #[test]
    fn stale_queued_hello_retries_fresh_peer_without_protocol_bypass() {
        use super::super::{
            tests::{pair, raw, scope},
            wire::{Body, Packet},
        };
        let identity = Identity {
            capabilities: 0,
            api_hash: "sdk".into(),
            artifact: "build".into(),
        };
        let mut scope = scope();
        scope.generation = 0;
        scope.sequence = 0;
        scope.lock_epoch = 0;
        scope.deadline_ns = 0;
        let (stale, stale_peer) = pair();
        raw(
            &stale_peer,
            &Packet {
                scope,
                body: Body::Hello(identity.clone()),
            }
            .encode()
            .unwrap(),
        );
        drop(stale_peer); // Native already expired its Hello budget while queued.
        let (fresh, fresh_peer) = pair();
        scope.nonce += 1;
        raw(
            &fresh_peer,
            &Packet {
                scope,
                body: Body::Hello(identity.clone()),
            }
            .encode()
            .unwrap(),
        );
        let mut transports = vec![fresh, stale];
        let deadline = monotonic_ns().unwrap() + 100_000_000;
        let bridge = retry_hello(deadline, || {
            Bridge::passive_handshake(
                transports.pop().unwrap(),
                &identity,
                Session::handshake_deadline(deadline)?,
            )
        })
        .unwrap();
        assert_eq!(bridge.connection_nonce(), scope.nonce);
        assert!(transports.is_empty());
        let peer = super::super::transport::Transport::from_owned_fd(fresh_peer);
        assert_eq!(
            peer.receive(deadline).unwrap().body,
            Body::HelloAck(identity)
        );
        for kind in [io::ErrorKind::InvalidData, io::ErrorKind::PermissionDenied] {
            let mut attempts = 0;
            assert!(
                retry_hello::<()>(deadline, || {
                    attempts += 1;
                    Err(io::Error::new(kind, "not transient"))
                })
                .is_err()
            );
            assert_eq!(attempts, 1);
        }
        assert!(
            retry_hello::<()>(monotonic_ns().unwrap(), || panic!(
                "expired admission must not attempt"
            ))
            .is_err()
        );
    }
    #[test]
    fn admission_budget_starts_after_slow_verification() {
        use std::time::Duration;
        let before = monotonic_ns().unwrap();
        let deadline = admission_deadline_after(Duration::from_millis(100), || {
            std::thread::sleep(Duration::from_millis(120));
            Ok(())
        })
        .unwrap();
        assert!(deadline >= before + 220_000_000);
        assert!(deadline > monotonic_ns().unwrap());
        assert!(admission_deadline_after(Duration::ZERO, || Ok(())).is_err());
        assert!(admission_deadline_after(Duration::from_secs(u64::MAX), || Ok(())).is_err());
        assert!(
            admission_deadline_after(Duration::from_millis(100), || Err(denied(
                "verification failed"
            )))
            .is_err()
        );
    }
    #[test]
    fn mapping_device_is_exact_hex_identity() {
        assert_eq!(parse_mapping_device("08:101"), Some((8, 257)));
        assert_eq!(parse_mapping_device("invalid"), None);
        assert_eq!(parse_mapping_device("08:zz"), None);
    }
    fn descriptor() -> InstalledDescriptor {
        InstalledDescriptor {
            wire_version: 1,
            qualification_revision: 0,
            build_id: format!("sledge-stock-v1:{}", "a".repeat(64)),
            compositor_path: "/nix/store/compositor/bin/Hyprland".into(),
            compositor_sha256: "b".repeat(64),
            sdk_api_hash: "sdk".into(),
            plugin_path: "/nix/store/plugin/plugin.so".into(),
            plugin_sha256: "c".repeat(64),
            capabilities: 0,
        }
    }
    #[test]
    fn exact_build_id_and_protocol_never_grant_qualification() {
        let mut d = descriptor();
        d.validate().unwrap();
        d.build_id = "stock-0.52.2:unknown".into();
        assert!(d.validate().is_err());
        d = descriptor();
        d.wire_version = 2;
        assert!(d.validate().is_err());
        d = descriptor();
        d.capabilities = 16;
        assert!(d.validate().is_err());
        d = descriptor();
        d.capabilities = 15;
        let metadata = VerifiedMetadata {
            descriptor: d,
            peer: Peer { uid: 0, pid: 1 },
            start_time: 1,
            descriptor_digest: "a".repeat(64),
        };
        assert_eq!(metadata.qualification(), Qualification::Unqualified);
        assert!(metadata.require_qualified().is_err());
        assert_eq!(metadata.passive_identity().capabilities, 0);
    }
    #[test]
    fn hashing_is_real_sha256_and_bounded_reads_reject_oversize() {
        let directory = PathBuf::from("/hdd/sledge-rust-production-r1");
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join(format!("hash-test-{}", std::process::id()));
        fs::write(&path, b"abc").unwrap();
        assert_eq!(
            hash_file(&path).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(bounded_read(&path, 2).is_err());
        assert!(immutable_file(&path).is_err());
        fs::remove_file(&path).unwrap();
    }
    #[test]
    fn proc_start_parser_handles_parentheses_in_name() {
        let fields = (0..20)
            .map(|n| {
                if n == 19 {
                    "1234".to_owned()
                } else {
                    "0".to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(
            parse_start(&format!("1 (name ) nested) {fields}")).unwrap(),
            1234
        );
    }
}
