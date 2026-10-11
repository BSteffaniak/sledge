//! Linux peer authorization. Socket credentials alone do not prove desktop authorization.
use nix::sys::socket::{getsockopt, sockopt::PeerCredentials};
use std::io;
use std::os::fd::{AsFd, AsRawFd};

/// Credentials obtained from the kernel, never from a protocol field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeerIdentity {
    pub uid: u32,
    pub pid: i32,
}

pub fn peer_identity(socket: &impl AsFd) -> io::Result<PeerIdentity> {
    let credentials = getsockopt(socket, PeerCredentials).map_err(io::Error::from)?;
    Ok(PeerIdentity {
        uid: credentials.uid(),
        pid: credentials.pid(),
    })
}

/// Authorize same-user IPC, including tokio sockets implementing `AsFd`.
/// This is NOT authorization for opening or grabbing a keyboard.
pub fn authorize_socket_peer(socket: &impl AsFd) -> io::Result<PeerIdentity> {
    let peer = peer_identity(socket)?;
    if peer.uid != nix::unistd::geteuid().as_raw() || peer.pid <= 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "IPC peer is not the daemon user",
        ));
    }
    Ok(peer)
}

/// Session facts must be independently read from a trusted desktop/session authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionProof {
    pub peer: PeerIdentity,
    pub seat: String,
    pub session: String,
    pub active: bool,
    pub unlocked: Option<bool>,
    pub local: bool,
}

impl SessionProof {
    /// Unknown lock state is deliberately denied, as are root and remote sessions.
    #[must_use]
    pub fn permits(&self, peer: PeerIdentity) -> bool {
        self.peer == peer
            && peer.uid != 0
            && peer.pid > 0
            && self.local
            && self.active
            && self.unlocked == Some(true)
            && !self.seat.is_empty()
            && !self.session.is_empty()
    }
}

/// Adapter must prove the peer belongs to the current local active unlocked seat.
/// It must not infer unlocked from an absent lock property or take compositor control.
pub trait SessionAuthority: Send {
    fn verify(&mut self, peer: PeerIdentity) -> io::Result<SessionProof>;

    /// Prove admission by the actual desktop consumer; a uinput node or delay
    /// alone is not readiness proof. Default denies until an adapter is qualified.
    fn verify_output_ready(&mut self, _fd: &dyn AsRawFd, _peer: PeerIdentity) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "verified output consumer readiness unavailable",
        ))
    }

    /// Independently verify this already-open input fd belongs to the authorized
    /// seat and is safe to proxy. Default denies: session membership alone is insufficient.
    fn verify_device(&mut self, _fd: &dyn AsRawFd, _peer: PeerIdentity) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "verified device/seat admission unavailable",
        ))
    }
}

/// Safe default until a qualified desktop authority is installed.
pub struct UnavailableAuthority;
impl SessionAuthority for UnavailableAuthority {
    fn verify(&mut self, _peer: PeerIdentity) -> io::Result<SessionProof> {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "verified session/seat/lock authority unavailable",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn kernel_credentials_and_unknown_lock_denied() {
        let (socket, _) = std::os::unix::net::UnixStream::pair().unwrap();
        let peer = authorize_socket_peer(&socket).unwrap();
        let mut proof = SessionProof {
            peer,
            seat: "seat0".into(),
            session: "test".into(),
            active: true,
            unlocked: None,
            local: true,
        };
        assert!(!proof.permits(peer));
        proof.unlocked = Some(true);
        assert_eq!(proof.permits(peer), peer.uid != 0);
        proof.active = false;
        assert!(!proof.permits(peer));
    }
}
