//! Fixed-size, versioned broker protocol. No paths, configuration, or arbitrary ioctl requests.
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

/// Aggregate frame deadline, not a fresh timeout for each fragment.
struct DeadlineSocket<'a> {
    socket: &'a mut UnixStream,
    deadline: Instant,
}
impl DeadlineSocket<'_> {
    fn remaining(&self) -> io::Result<Duration> {
        self.deadline
            .checked_duration_since(Instant::now())
            .filter(|left| !left.is_zero())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::TimedOut, "broker frame deadline exceeded")
            })
    }
}
impl Read for DeadlineSocket<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.socket.set_read_timeout(Some(self.remaining()?))?;
        self.socket.read(bytes)
    }
}
impl Write for DeadlineSocket<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.socket.set_write_timeout(Some(self.remaining()?))?;
        self.socket.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.socket.flush()
    }
}

/// Maximum decision time; late replies must not affect another event.
pub const DECISION_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(10);
const MAGIC: [u8; 4] = *b"SLG1";
const SIZE: usize = 40;

/// Raw Linux event fields, independent of native timeval layout.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RawEvent {
    pub event_type: u16,
    pub code: u16,
    pub value: i32,
}

/// Protocol operation. Replacement is reserved until transactional synthesis is qualified.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Operation {
    Event = 1,
    Pass = 2,
    Swallow = 3,
    /// Reserved unsupported opcode. Privileged admission must reject it.
    Replace = 4,
    DeviceReset = 5,
    SnapshotKey = 6,
    DeviceRetire = 7,
}

/// Every reply must echo the device generation, state epoch and sequence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Message {
    pub operation: Operation,
    pub generation: u64,
    pub epoch: u64,
    pub sequence: u64,
    pub event: RawEvent,
}

impl Message {
    pub fn read_socket(socket: &mut UnixStream, deadline: Instant) -> io::Result<Self> {
        Self::read(&mut DeadlineSocket { socket, deadline })
    }
    pub fn write_socket(self, socket: &mut UnixStream, deadline: Instant) -> io::Result<()> {
        self.write(&mut DeadlineSocket { socket, deadline })
    }
    pub fn write(self, writer: &mut impl Write) -> io::Result<()> {
        let mut bytes = [0; SIZE];
        bytes[..4].copy_from_slice(&MAGIC);
        bytes[4] = self.operation as u8;
        bytes[8..16].copy_from_slice(&self.generation.to_le_bytes());
        bytes[16..24].copy_from_slice(&self.epoch.to_le_bytes());
        bytes[24..32].copy_from_slice(&self.sequence.to_le_bytes());
        bytes[32..34].copy_from_slice(&self.event.event_type.to_le_bytes());
        bytes[34..36].copy_from_slice(&self.event.code.to_le_bytes());
        bytes[36..40].copy_from_slice(&self.event.value.to_le_bytes());
        writer.write_all(&bytes)
    }

    pub fn read(reader: &mut impl Read) -> io::Result<Self> {
        let mut bytes = [0; SIZE];
        reader.read_exact(&mut bytes)?;
        if bytes[..4] != MAGIC || bytes[5..8] != [0; 3] {
            return Err(invalid());
        }
        let operation = match bytes[4] {
            1 => Operation::Event,
            2 => Operation::Pass,
            3 => Operation::Swallow,
            4 => Operation::Replace,
            5 => Operation::DeviceReset,
            6 => Operation::SnapshotKey,
            7 => Operation::DeviceRetire,
            _ => return Err(invalid()),
        };
        Ok(Self {
            operation,
            generation: u64::from_le_bytes(bytes[8..16].try_into().map_err(|_| invalid())?),
            epoch: u64::from_le_bytes(bytes[16..24].try_into().map_err(|_| invalid())?),
            sequence: u64::from_le_bytes(bytes[24..32].try_into().map_err(|_| invalid())?),
            event: RawEvent {
                event_type: u16::from_le_bytes([bytes[32], bytes[33]]),
                code: u16::from_le_bytes([bytes[34], bytes[35]]),
                value: i32::from_le_bytes(bytes[36..40].try_into().map_err(|_| invalid())?),
            },
        })
    }

    #[must_use]
    pub fn answers(self, request: Self) -> bool {
        matches!(
            self.operation,
            Operation::Pass | Operation::Swallow | Operation::Replace
        ) && self.generation == request.generation
            && self.epoch == request.epoch
            && self.sequence == request.sequence
    }
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid sledge broker frame")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_roundtrip_and_generation_guard() {
        let request = Message {
            operation: Operation::Event,
            generation: 4,
            epoch: 5,
            sequence: 6,
            event: RawEvent {
                event_type: 1,
                code: 30,
                value: 2,
            },
        };
        let mut bytes = Vec::new();
        request.write(&mut bytes).unwrap();
        assert_eq!(bytes.len(), SIZE);
        assert_eq!(Message::read(&mut bytes.as_slice()).unwrap(), request);
        let mut reply = Message {
            operation: Operation::Pass,
            ..request
        };
        assert!(reply.answers(request));
        reply.epoch += 1;
        assert!(!reply.answers(request));
        bytes[5] = 1;
        assert!(Message::read(&mut bytes.as_slice()).is_err());
    }
    #[test]
    fn socket_deadline_rejects_partial_frame() {
        let (mut a, mut b) = std::os::unix::net::UnixStream::pair().unwrap();
        a.write_all(b"SLG1").unwrap();
        let start = std::time::Instant::now();
        assert!(
            Message::read_socket(&mut b, start + std::time::Duration::from_millis(20)).is_err()
        );
        assert!(start.elapsed() < std::time::Duration::from_secs(1));
    }

    #[test]
    fn isolated_unix_transport() {
        let (mut a, mut b) = std::os::unix::net::UnixStream::pair().unwrap();
        let message = Message {
            operation: Operation::Pass,
            generation: 1,
            epoch: 1,
            sequence: 1,
            event: RawEvent::default(),
        };
        message.write(&mut a).unwrap();
        assert_eq!(Message::read(&mut b).unwrap(), message);
    }
}
