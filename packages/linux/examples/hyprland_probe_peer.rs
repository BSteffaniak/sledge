//! Isolated-guest passive SHP1 listener. Explicit endpoint/identity, no discovery.
#[cfg(target_os = "linux")]
mod linux {
    use sledge_linux::hyprland::Bridge;
    use sledge_linux::hyprland::listener::Listener;
    use sledge_linux::hyprland::transport::{Peer, monotonic_ns};
    use sledge_linux::hyprland::wire::Identity;
    use std::io;
    use std::path::PathBuf;

    struct Arguments {
        endpoint: PathBuf,
        pid: i32,
        identity: Identity,
        total_ms: u64,
    }
    fn invalid() -> io::Error {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: hyprland_probe_peer ABS_ENDPOINT COMPOSITOR_PID API_HASH ARTIFACT_ID TOTAL_MS(1..60000)",
        )
    }
    fn arguments() -> io::Result<Arguments> {
        let args = std::env::args().skip(1).collect::<Vec<_>>();
        if args.len() != 5 {
            return Err(invalid());
        }
        let endpoint = PathBuf::from(&args[0]);
        let pid = args[1].parse::<i32>().map_err(|_| invalid())?;
        let total_ms = args[4].parse::<u64>().map_err(|_| invalid())?;
        if !endpoint.is_absolute()
            || pid <= 0
            || !(1..=60000).contains(&total_ms)
            || [args[2].as_str(), args[3].as_str()]
                .iter()
                .any(|v| v.is_empty() || v.len() > 192 || v.contains('\0'))
        {
            return Err(invalid());
        }
        Ok(Arguments {
            endpoint,
            pid,
            identity: Identity {
                capabilities: 0,
                api_hash: args[2].clone(),
                artifact: args[3].clone(),
            },
            total_ms,
        })
    }
    pub fn run() -> io::Result<()> {
        let args = arguments()?;
        let started = monotonic_ns()?;
        let total_deadline = started
            .checked_add(args.total_ms * 1_000_000)
            .ok_or_else(invalid)?;
        let listener = Listener::bind(&args.endpoint)?;
        // Start this listener before loading the native passive plugin. No
        // readiness output in the native 10ms handshake transaction itself.
        println!("probe listening capabilities=0 capture=false");
        let transport = listener.accept(
            Peer {
                uid: nix::unistd::geteuid().as_raw(),
                pid: args.pid,
            },
            total_deadline,
        )?;
        let handshake_deadline = total_deadline.min(monotonic_ns()? + 10_000_000);
        // Native sends Hello with global scope/deadline zero. Exact echo only.
        let _bridge = Bridge::passive_handshake(transport, &args.identity, handshake_deadline)?;
        println!(
            "probe handshakes=1 events=0 capabilities=0 capture=false elapsed_ns={} status=handshake_only",
            monotonic_ns()?.saturating_sub(started)
        );
        // Current native passive plugin exports no events. Exit successfully;
        // this is transport identity evidence, not capture/delivery qualification.
        Ok(())
    }
}
#[cfg(target_os = "linux")]
fn main() -> std::process::ExitCode {
    match linux::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("passive probe failed (no retry): {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
#[cfg(not(target_os = "linux"))]
fn main() -> std::process::ExitCode {
    eprintln!("hyprland_probe_peer requires Linux");
    std::process::ExitCode::FAILURE
}
