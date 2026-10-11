//! System-bus logind session inspection. LockedHint is not treated as unlock proof.
use crate::authorization::{PeerIdentity, SessionAuthority, SessionProof};
use std::io;
use std::time::Duration;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::OwnedObjectPath;

pub struct LogindAuthority {
    connection: Connection,
}
impl LogindAuthority {
    pub fn connect() -> io::Result<Self> {
        let connection = zbus::blocking::connection::Builder::system()
            .map_err(bus_error)?
            .method_timeout(Duration::from_millis(250))
            .build()
            .map_err(bus_error)?;
        Ok(Self { connection })
    }
    /// Read actual peer session properties from the privileged system bus.
    /// A positive LockedHint denies immediately; a negative hint remains unknown.
    pub fn inspect(&self, peer: PeerIdentity) -> io::Result<SessionProof> {
        let pid = u32::try_from(peer.pid).map_err(bus_error)?;
        let manager = Proxy::new(
            &self.connection,
            "org.freedesktop.login1",
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
        )
        .map_err(bus_error)?;
        let path: OwnedObjectPath = manager
            .call("GetSessionByPID", &(pid,))
            .map_err(bus_error)?;
        let session = Proxy::new(
            &self.connection,
            "org.freedesktop.login1",
            path.as_str(),
            "org.freedesktop.login1.Session",
        )
        .map_err(bus_error)?;
        let (uid, _): (u32, OwnedObjectPath) = session.get_property("User").map_err(bus_error)?;
        if uid != peer.uid {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "peer does not own logind session",
            ));
        }
        let (seat, _): (String, OwnedObjectPath) =
            session.get_property("Seat").map_err(bus_error)?;
        let id: String = session.get_property("Id").map_err(bus_error)?;
        let active: bool = session.get_property("Active").map_err(bus_error)?;
        let remote: bool = session.get_property("Remote").map_err(bus_error)?;
        let kind: String = session.get_property("Type").map_err(bus_error)?;
        let locked: bool = session.get_property("LockedHint").map_err(bus_error)?;
        Ok(SessionProof {
            peer,
            seat,
            session: id,
            active,
            unlocked: locked.then_some(false),
            local: !remote && matches!(kind.as_str(), "wayland" | "x11"),
        })
    }
}
impl SessionAuthority for LogindAuthority {
    fn verify(&mut self, peer: PeerIdentity) -> io::Result<SessionProof> {
        self.inspect(peer)
    }
    // Device admission remains denied: opening a seat's keyboard is not proof that
    // a compositor has yielded ownership, and LockedHint=false is not lock proof.
}
fn bus_error(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(format!("logind: {error}"))
}
