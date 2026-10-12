//! Frozen SHP1 proposal framing. This is not a qualified capture protocol.
use std::io;

pub const PACKET_SIZE: usize = 512;
pub const MAX_WAIT_NS: u64 = 10_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scope {
    pub generation: u64,
    pub lock_epoch: u64,
    pub sequence: u64,
    pub deadline_ns: u64,
    pub nonce: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub capabilities: u64,
    pub api_hash: String,
    pub artifact: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub key: u32,
    pub state: u32,
    pub time_ms: u32,
    pub modifiers: u32,
    pub focus_epoch: u64,
    pub focus_kind: u16,
    pub focus: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Pass,
    Swallow,
    Replace { key: u32, modifiers: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum ResultKind {
    Replaced = 0,
    RejectedOriginalAdmitted = 1,
    PartialUncertainFatal = 2,
    TimeoutOriginalAdmitted = 3,
    SwallowCommitted = 4,
    StockConsumed = 5,
    OriginalAdmitted = 6,
    MergedOriginal = 7,
    LockerOwned = 8,
    Quarantined = 9,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub result: ResultKind,
    pub reason: u32,
    pub key: u32,
    pub state: u32,
    pub focus_epoch: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Body {
    Hello(Identity),
    HelloAck(Identity),
    Event(Event),
    Decision(Decision),
    Outcome(Outcome),
    Inject(Decision),
    Reset(u32),
    Retire(u32),
    Lock { state: u32, reason: u32 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Packet {
    pub scope: Scope,
    pub body: Body,
}

pub(crate) fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid Hyprland SHP1 packet/scope",
    )
}
fn u16_at(b: &[u8], p: usize) -> u16 {
    u16::from_le_bytes(b[p..p + 2].try_into().expect("fixed field"))
}
fn u32_at(b: &[u8], p: usize) -> u32 {
    u32::from_le_bytes(b[p..p + 4].try_into().expect("fixed field"))
}
fn u64_at(b: &[u8], p: usize) -> u64 {
    u64::from_le_bytes(b[p..p + 8].try_into().expect("fixed field"))
}
fn zero(b: &[u8]) -> io::Result<()> {
    if b.iter().any(|v| *v != 0) {
        Err(invalid())
    } else {
        Ok(())
    }
}
fn text(b: &[u8]) -> io::Result<String> {
    if b.contains(&0) {
        return Err(invalid());
    }
    std::str::from_utf8(b)
        .map(str::to_owned)
        .map_err(|_| invalid())
}
fn hello(b: &[u8]) -> io::Result<Identity> {
    let h = usize::from(u16_at(b, 8));
    let a = usize::from(u16_at(b, 10));
    let capabilities = u64_at(b, 0);
    if h > 192 || a > 192 || capabilities & !15 != 0 {
        return Err(invalid());
    }
    zero(&b[12..16])?;
    zero(&b[16 + h..208])?;
    zero(&b[208 + a..400])?;
    Ok(Identity {
        capabilities,
        api_hash: text(&b[16..16 + h])?,
        artifact: text(&b[208..208 + a])?,
    })
}
fn event(b: &[u8]) -> io::Result<Event> {
    let len = usize::from(u16_at(b, 26));
    let kind = u16_at(b, 24);
    if len > 256
        || b.len() != 32 + len
        || kind > 2
        || (kind == 0 && len != 0)
        || (kind != 0 && len == 0)
        || u32_at(b, 4) > 2
        || u32_at(b, 12) & !255 != 0
    {
        return Err(invalid());
    }
    zero(&b[28..32])?;
    Ok(Event {
        key: u32_at(b, 0),
        state: u32_at(b, 4),
        time_ms: u32_at(b, 8),
        modifiers: u32_at(b, 12),
        focus_epoch: u64_at(b, 16),
        focus_kind: kind,
        focus: text(&b[32..])?,
    })
}
fn decision(b: &[u8]) -> io::Result<Decision> {
    zero(&b[12..16])?;
    let key = u32_at(b, 4);
    let modifiers = u32_at(b, 8);
    match u32_at(b, 0) {
        0 if key == 0 && modifiers == 0 => Ok(Decision::Pass),
        1 if key == 0 && modifiers == 0 => Ok(Decision::Swallow),
        2 if modifiers & !255 == 0 => Ok(Decision::Replace { key, modifiers }),
        _ => Err(invalid()),
    }
}
fn outcome(b: &[u8]) -> io::Result<Outcome> {
    let result = match u32_at(b, 0) {
        0 => ResultKind::Replaced,
        1 => ResultKind::RejectedOriginalAdmitted,
        2 => ResultKind::PartialUncertainFatal,
        3 => ResultKind::TimeoutOriginalAdmitted,
        4 => ResultKind::SwallowCommitted,
        5 => ResultKind::StockConsumed,
        6 => ResultKind::OriginalAdmitted,
        7 => ResultKind::MergedOriginal,
        8 => ResultKind::LockerOwned,
        9 => ResultKind::Quarantined,
        _ => return Err(invalid()),
    };
    if u32_at(b, 4) > 7 || u32_at(b, 12) > 2 {
        return Err(invalid());
    }
    Ok(Outcome {
        result,
        reason: u32_at(b, 4),
        key: u32_at(b, 8),
        state: u32_at(b, 12),
        focus_epoch: u64_at(b, 16),
    })
}

impl Packet {
    /// Strict decode including all reserved fields and unused slots.
    /// # Errors
    /// Rejects noncanonical framing, fields, UTF-8 and padding.
    pub fn decode(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() != PACKET_SIZE || bytes[..4] != *b"SHP1" || u16_at(bytes, 4) != 1 {
            return Err(invalid());
        }
        let len = usize::try_from(u32_at(bytes, 8)).map_err(|_| invalid())?;
        if len > 448 {
            return Err(invalid());
        }
        zero(&bytes[12..16])?;
        zero(&bytes[56..64])?;
        zero(&bytes[64 + len..])?;
        let b = &bytes[64..64 + len];
        let scope = Scope {
            generation: u64_at(bytes, 16),
            lock_epoch: u64_at(bytes, 24),
            sequence: u64_at(bytes, 32),
            deadline_ns: u64_at(bytes, 40),
            nonce: u64_at(bytes, 48),
        };
        let body = match u16_at(bytes, 6) {
            1 if len == 400 => Body::Hello(hello(b)?),
            2 if len == 400 => Body::HelloAck(hello(b)?),
            3 if len >= 32 => Body::Event(event(b)?),
            4 if len == 16 => Body::Decision(decision(b)?),
            5 if len == 24 => Body::Outcome(outcome(b)?),
            6 if len == 16 && scope.generation == 0 => {
                let d = decision(b)?;
                if !matches!(d, Decision::Replace { .. }) {
                    return Err(invalid());
                }
                Body::Inject(d)
            }
            7 | 8 if len == 8 => {
                zero(&b[4..])?;
                let reason = u32_at(b, 0);
                if reason > 7 {
                    return Err(invalid());
                }
                if u16_at(bytes, 6) == 7 {
                    Body::Reset(reason)
                } else if scope.generation != 0 {
                    Body::Retire(reason)
                } else {
                    return Err(invalid());
                }
            }
            9 if len == 8 && u32_at(b, 0) <= 2 && u32_at(b, 4) <= 7 => Body::Lock {
                state: u32_at(b, 0),
                reason: u32_at(b, 4),
            },
            _ => return Err(invalid()),
        };
        Ok(Self { scope, body })
    }

    /// Encode a canonical packet, rejecting invalid values rather than truncating.
    /// # Errors
    /// Returns invalid data for values outside the frozen wire contract.
    pub fn encode(&self) -> io::Result<[u8; PACKET_SIZE]> {
        let mut bytes = [0; PACKET_SIZE];
        bytes[..4].copy_from_slice(b"SHP1");
        put(&mut bytes, 4, &1_u16.to_le_bytes());
        for (offset, value) in [
            (16, self.scope.generation),
            (24, self.scope.lock_epoch),
            (32, self.scope.sequence),
            (40, self.scope.deadline_ns),
            (48, self.scope.nonce),
        ] {
            put(&mut bytes, offset, &value.to_le_bytes());
        }
        let (opcode, body) = encode_body(&self.body)?;
        put(&mut bytes, 6, &opcode.to_le_bytes());
        put(
            &mut bytes,
            8,
            &u32::try_from(body.len())
                .map_err(|_| invalid())?
                .to_le_bytes(),
        );
        put(&mut bytes, 64, &body);
        Self::decode(&bytes)?;
        Ok(bytes)
    }
}
fn put(b: &mut [u8], p: usize, value: &[u8]) {
    b[p..p + value.len()].copy_from_slice(value);
}
fn encode_body(body: &Body) -> io::Result<(u16, Vec<u8>)> {
    let (opcode, len) = match body {
        Body::Hello(_) => (1, 400),
        Body::HelloAck(_) => (2, 400),
        Body::Event(e) if e.focus.len() <= 256 => (3, 32 + e.focus.len()),
        Body::Decision(_) => (4, 16),
        Body::Outcome(_) => (5, 24),
        Body::Inject(_) => (6, 16),
        Body::Reset(_) => (7, 8),
        Body::Retire(_) => (8, 8),
        Body::Lock { .. } => (9, 8),
        Body::Event(_) => return Err(invalid()),
    };
    let mut b = vec![0; len];
    match body {
        Body::Hello(i) | Body::HelloAck(i) => {
            if i.api_hash.len() > 192 || i.artifact.len() > 192 {
                return Err(invalid());
            }
            put(&mut b, 0, &i.capabilities.to_le_bytes());
            put(
                &mut b,
                8,
                &u16::try_from(i.api_hash.len())
                    .map_err(|_| invalid())?
                    .to_le_bytes(),
            );
            put(
                &mut b,
                10,
                &u16::try_from(i.artifact.len())
                    .map_err(|_| invalid())?
                    .to_le_bytes(),
            );
            put(&mut b, 16, i.api_hash.as_bytes());
            put(&mut b, 208, i.artifact.as_bytes());
        }
        Body::Event(e) => {
            for (p, v) in [(0, e.key), (4, e.state), (8, e.time_ms), (12, e.modifiers)] {
                put(&mut b, p, &v.to_le_bytes());
            }
            put(&mut b, 16, &e.focus_epoch.to_le_bytes());
            put(&mut b, 24, &e.focus_kind.to_le_bytes());
            put(
                &mut b,
                26,
                &u16::try_from(e.focus.len())
                    .map_err(|_| invalid())?
                    .to_le_bytes(),
            );
            put(&mut b, 32, e.focus.as_bytes());
        }
        Body::Decision(d) | Body::Inject(d) => {
            let (verdict, key, modifiers) = match d {
                Decision::Pass => (0_u32, 0_u32, 0_u32),
                Decision::Swallow => (1, 0, 0),
                Decision::Replace { key, modifiers } => (2, *key, *modifiers),
            };
            for (p, v) in [(0, verdict), (4, key), (8, modifiers)] {
                put(&mut b, p, &v.to_le_bytes());
            }
        }
        Body::Outcome(o) => {
            for (p, v) in [
                (0, o.result as u32),
                (4, o.reason),
                (8, o.key),
                (12, o.state),
            ] {
                put(&mut b, p, &v.to_le_bytes());
            }
            put(&mut b, 16, &o.focus_epoch.to_le_bytes());
        }
        Body::Reset(r) | Body::Retire(r) => put(&mut b, 0, &r.to_le_bytes()),
        Body::Lock { state, reason } => {
            put(&mut b, 0, &state.to_le_bytes());
            put(&mut b, 4, &reason.to_le_bytes());
        }
    }
    Ok((opcode, b))
}
