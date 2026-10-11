//! Privileged broker entry point. A trusted launcher supplies a pre-bound local
//! Unix listener; this executable never installs a service or reads user config.
#[cfg(target_os = "linux")]
fn serve() -> std::io::Result<()> {
    use sledge_linux::{
        authorization::SessionAuthority, lifecycle::ProxyLifecycle, logind::LogindAuthority,
    };
    use std::os::fd::{FromRawFd, OwnedFd};
    if !nix::unistd::geteuid().is_root() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "broker requires narrowly privileged launcher",
        ));
    }
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    if arguments == ["--check"] {
        LogindAuthority::connect()?;
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "system bus reachable; strict lock/device ownership authority unqualified; no devices opened",
        ));
    }
    if arguments.len() != 2 || arguments[0] != "--listen-fd" {
        return Err(std::io::Error::other(
            "usage: sledge-linux-broker --listen-fd FD (trusted pre-bound Unix listener; strict lock authority still required)",
        ));
    }
    let fd: i32 = arguments[1].parse().map_err(std::io::Error::other)?;
    if fd < 3 {
        return Err(std::io::Error::other(
            "listener must not be a standard stream",
        ));
    }
    let duplicate = nix::fcntl::fcntl(fd, nix::fcntl::FcntlArg::F_DUPFD_CLOEXEC(3))
        .map_err(std::io::Error::from)?;
    // SAFETY: fcntl returned a newly owned descriptor, transferred once.
    let owned = unsafe { OwnedFd::from_raw_fd(duplicate) };
    if !nix::sys::socket::getsockopt(&owned, nix::sys::socket::sockopt::AcceptConn)
        .map_err(std::io::Error::from)?
    {
        return Err(std::io::Error::other("fd is not a listening socket"));
    }
    let listener = std::os::unix::net::UnixListener::from(owned);
    listener.local_addr()?;
    let mut authority = LogindAuthority::connect()?;
    for client in listener.incoming() {
        let mut client = client?;
        let peer = sledge_linux::authorization::peer_identity(&client)?;
        let proof = match authority.verify(peer) {
            Ok(proof) if proof.permits(peer) => proof,
            _ => {
                eprintln!(
                    "sledge-linux-broker: peer lacks verified active local unlocked session; capture refused"
                );
                continue;
            }
        };
        // No device discovery/open occurs until the independently verified gate succeeds.
        let mut lifecycle = ProxyLifecycle::default();
        eprintln!("sledge-linux-broker: authorized seat {}", proof.seat);
        while lifecycle.step(&mut client, &mut authority).is_ok() {
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        lifecycle.bypass();
    }
    Ok(())
}
#[cfg(target_os = "linux")]
fn main() -> std::process::ExitCode {
    match serve() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("sledge-linux-broker: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
#[cfg(not(target_os = "linux"))]
fn main() -> std::process::ExitCode {
    eprintln!("sledge-linux-broker is only available on Linux");
    std::process::ExitCode::FAILURE
}
