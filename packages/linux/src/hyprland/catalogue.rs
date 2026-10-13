//! Compile-pinned candidate admission. Normal builds embed no catalogue.
//! Scope markers are test-package guards, NOT sandbox/VM cryptographic proof.
use super::session::{Qualification, VerifiedMetadata};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::os::unix::fs::MetadataExt;
use std::{fs, io, path::PathBuf};

pub const FRESH_DOWN_SEND_KEY: u64 = 1;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    policy_revision: u64,
    entries: Vec<Entry>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    descriptor_sha256: String,
    build_id: String,
    evidence_sha256: String,
    feature_mask: u64,
    status: String,
    isolated_vm: bool,
    machine_id_sha256: String,
    boot_marker_path: PathBuf,
    boot_marker_sha256: String,
}
/// Opaque result from embedded policy + verified actual metadata + scope guard.
pub struct AdmittedMetadata {
    pub(super) metadata: VerifiedMetadata,
    pub(super) feature_mask: u64,
}
impl AdmittedMetadata {
    #[must_use]
    pub const fn qualification(&self) -> Qualification {
        Qualification::Candidate
    }
    #[must_use]
    pub const fn feature_mask(&self) -> u64 {
        self.feature_mask
    }
    /// Candidates are never supported host registrations.
    /// # Errors
    /// Always rejects Candidate; accepted policy remains disabled.
    pub fn require_qualified(&self) -> io::Result<()> {
        Err(denied("candidate is not supported host qualification"))
    }
}
pub struct Catalogue;
impl Catalogue {
    pub(super) fn recheck(metadata: &VerifiedMetadata) -> io::Result<()> {
        let bytes = include_bytes!(concat!(env!("OUT_DIR"), "/hyprland-catalogue.json"));
        let entry = select(
            bytes,
            env!("SLEDGE_EMBEDDED_CATALOGUE_SHA256"),
            &metadata.descriptor_digest,
            &metadata.descriptor().build_id,
        )?;
        verify_scope(&entry)
    }
    /// Admit using ONLY embedded policy. No runtime policy path or trusted bool.
    /// # Errors
    /// Rejects absent/bad pin, schema/revision/entry mismatch and wrong scope.
    pub fn qualify(metadata: VerifiedMetadata) -> io::Result<AdmittedMetadata> {
        let bytes = include_bytes!(concat!(env!("OUT_DIR"), "/hyprland-catalogue.json"));
        if metadata.descriptor().qualification_revision != 1 {
            return Err(denied(
                "candidate descriptor qualification revision must be 1",
            ));
        }
        let entry = select(
            bytes,
            env!("SLEDGE_EMBEDDED_CATALOGUE_SHA256"),
            &metadata.descriptor_digest,
            &metadata.descriptor().build_id,
        )?;
        verify_scope(&entry)?;
        Ok(AdmittedMetadata {
            metadata,
            feature_mask: entry.feature_mask,
        })
    }
}
fn denied(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn select(bytes: &[u8], pin: &str, descriptor: &str, build: &str) -> io::Result<Entry> {
    if bytes.is_empty() || bytes.len() > 65536 || !hex(pin) || digest(bytes) != pin {
        return Err(denied("missing/mismatched compile-pinned catalogue"));
    }
    let policy: Policy =
        serde_json::from_slice(bytes).map_err(|_| denied("invalid catalogue schema"))?;
    if policy.policy_revision != 1 || policy.entries.len() > 16 {
        return Err(denied("unsupported catalogue policy revision/budget"));
    }
    let mut entries = policy
        .entries
        .into_iter()
        .filter(|e| e.descriptor_sha256 == descriptor && e.build_id == build);
    let entry = entries
        .next()
        .ok_or_else(|| denied("descriptor not in compiled catalogue"))?;
    if entries.next().is_some() {
        return Err(denied("ambiguous catalogue descriptor"));
    }
    if entry.status != "candidate"
        || !entry.isolated_vm
        || entry.feature_mask != FRESH_DOWN_SEND_KEY
        || !hex(&entry.descriptor_sha256)
        || !hex(&entry.evidence_sha256)
        || !hex(&entry.machine_id_sha256)
        || !hex(&entry.boot_marker_sha256)
        || !entry.boot_marker_path.is_absolute()
    {
        return Err(denied("unsupported candidate status/features/scope"));
    }
    Ok(entry)
}
fn guarded_bytes(path: &std::path::Path) -> io::Result<Vec<u8>> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
        .open(path)?;
    let m = file.metadata()?;
    if !m.is_file() || m.uid() != 0 || m.mode() & 0o222 != 0 || m.len() > 4096 {
        return Err(denied(
            "candidate scope guard is mutable/oversized/not root-owned",
        ));
    }
    let mut bytes = Vec::new();
    (&mut file).take(4097).read_to_end(&mut bytes)?;
    if bytes.len() > 4096 {
        return Err(denied("candidate scope guard oversized"));
    }
    Ok(bytes)
}
fn verify_scope(e: &Entry) -> io::Result<()> {
    if digest(&guarded_bytes(std::path::Path::new("/etc/machine-id"))?) != e.machine_id_sha256
        || digest(&guarded_bytes(&e.boot_marker_path)?) != e.boot_marker_sha256
    {
        return Err(denied("candidate VM scope mismatch"));
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalogue_pins_exact_descriptor_revision_features_and_candidate_status() {
        let d = "a".repeat(64);
        let b = format!("sledge-stock-v1:{}", "b".repeat(64));
        let value = serde_json::json!({"policy_revision":1,"entries":[{"descriptor_sha256":d,"build_id":b,"evidence_sha256":"c".repeat(64),"feature_mask":1,"status":"candidate","isolated_vm":true,"machine_id_sha256":"d".repeat(64),"boot_marker_path":"/etc/sledge-vm-marker","boot_marker_sha256":"e".repeat(64)}]});
        let bytes = serde_json::to_vec(&value).unwrap();
        assert!(select(&bytes, &digest(&bytes), &d, &b).is_ok());
        assert!(select(&bytes, &"0".repeat(64), &d, &b).is_err());
        assert!(select(&bytes, &digest(&bytes), &"f".repeat(64), &b).is_err());
        for (field, replacement) in [
            ("feature_mask", serde_json::json!(15)),
            ("status", serde_json::json!("supported")),
            ("isolated_vm", serde_json::json!(false)),
        ] {
            let mut bad = value.clone();
            bad["entries"][0][field] = replacement;
            let bytes = serde_json::to_vec(&bad).unwrap();
            assert!(select(&bytes, &digest(&bytes), &d, &b).is_err());
        }
        let mut bad = value;
        bad["policy_revision"] = serde_json::json!(2);
        let bytes = serde_json::to_vec(&bad).unwrap();
        assert!(select(&bytes, &digest(&bytes), &d, &b).is_err());
    }
}
