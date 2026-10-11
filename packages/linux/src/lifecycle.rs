//! Broker-owned discovery and proxy lifecycle. Discovery does not open input devices.
use crate::authorization::{PeerIdentity, SessionAuthority};
use crate::broker::DeviceProxy;
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read};
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceIdentity {
    pub path: PathBuf,
    pub device: u64,
    pub inode: u64,
    pub rdev: u64,
    pub seat: String,
}

/// Require root-owned non-writable udev database proof, not a client seat string.
fn seat_for(rdev: u64) -> io::Result<String> {
    let path = format!(
        "/run/udev/data/c{}:{}",
        nix::sys::stat::major(rdev),
        nix::sys::stat::minor(rdev)
    );
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
        return Err(denied("untrusted udev seat database"));
    }
    let mut data = String::new();
    file.by_ref().take(65_537).read_to_string(&mut data)?;
    if data.len() > 65_536 {
        return Err(denied("udev record exceeds bound"));
    }
    if !data.lines().any(|line| line == "E:ID_INPUT_KEYBOARD=1") {
        return Err(denied("udev device is not a keyboard"));
    }
    let seat = data
        .lines()
        .find_map(|line| line.strip_prefix("E:ID_SEAT="))
        .unwrap_or("seat0");
    if seat.is_empty()
        || seat.len() > 128
        || !seat
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(denied("invalid seat"));
    }
    Ok(seat.to_owned())
}

/// Fixed broker-controlled /dev/input discovery; missing seat evidence omits a device.
pub fn discover_keyboards() -> io::Result<Vec<DeviceIdentity>> {
    let mut devices = Vec::new();
    for entry in fs::read_dir("/dev/input")? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(number) = name.strip_prefix("event") else {
            continue;
        };
        if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.file_type().is_char_device() || nix::sys::stat::major(metadata.rdev()) != 13 {
            continue;
        }
        if let Ok(seat) = seat_for(metadata.rdev()) {
            devices.push(DeviceIdentity {
                path,
                device: metadata.dev(),
                inode: metadata.ino(),
                rdev: metadata.rdev(),
                seat,
            });
        }
        if devices.len() > 64 {
            return Err(denied("keyboard discovery exceeds bound"));
        }
    }
    Ok(devices)
}

/// Periodically reconcile hotplug with monotonically increasing generations.
#[derive(Default)]
pub struct ProxyLifecycle {
    proxies: BTreeMap<PathBuf, (DeviceIdentity, DeviceProxy)>,
    generation: u64,
}
impl ProxyLifecycle {
    pub fn reconcile(
        &mut self,
        peer: PeerIdentity,
        client: &mut UnixStream,
        authority: &mut dyn SessionAuthority,
    ) -> io::Result<()> {
        let proof = authority.verify(peer)?;
        if !proof.permits(peer) {
            self.proxies.clear();
            return Err(denied("strict session authorization unavailable"));
        }
        let candidates = discover_keyboards()?;
        let removed: Vec<_> = self
            .proxies
            .iter()
            .filter(|(_, (identity, _))| {
                !candidates.contains(identity) || identity.seat != proof.seat
            })
            .map(|(path, _)| path.clone())
            .collect();
        for path in removed {
            if let Some((_, mut proxy)) = self.proxies.remove(&path) {
                proxy.bypass();
                proxy.retire_client(client)?;
            }
        }
        for identity in candidates
            .into_iter()
            .filter(|identity| identity.seat == proof.seat)
        {
            if self.proxies.contains_key(&identity.path) {
                continue;
            }
            let file = fs::OpenOptions::new()
                .read(true)
                .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC | nix::libc::O_NONBLOCK)
                .open(&identity.path)?;
            let metadata = file.metadata()?;
            if metadata.dev() != identity.device
                || metadata.ino() != identity.inode
                || metadata.rdev() != identity.rdev
            {
                return Err(denied("input identity changed during admission"));
            }
            self.generation = self
                .generation
                .checked_add(1)
                .ok_or_else(|| denied("generation exhausted"))?;
            let mut proxy =
                DeviceProxy::from_authorized_fd(file.into(), self.generation, peer, authority)?;
            proxy.activate(peer, authority)?;
            proxy.reset_client(client)?;
            self.proxies
                .insert(identity.path.clone(), (identity, proxy));
        }
        Ok(())
    }
    /// Any failure tears down every proxy; never leave another keyboard grabbed.
    pub fn step(
        &mut self,
        client: &mut UnixStream,
        authority: &mut dyn SessionAuthority,
    ) -> io::Result<()> {
        let result = (|| {
            let peer = crate::authorization::peer_identity(client)?;
            self.reconcile(peer, client, authority)?;
            for (_, proxy) in self.proxies.values_mut() {
                proxy.pump(client, authority)?;
            }
            Ok(())
        })();
        if result.is_err() {
            self.proxies.clear();
        }
        result
    }
    pub fn bypass(&mut self) {
        self.proxies.clear();
    }
}
fn denied(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}

/// Verify operator-provided socket parent without parsing any user configuration.
pub fn verify_broker_directory(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
        return Err(denied(
            "broker socket directory must be root-owned and non-writable",
        ));
    }
    Ok(())
}
