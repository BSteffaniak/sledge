//! Compile-pinned admission plus independent compile-pinned Supported acceptance.
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
    entries: Vec<Admission>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    descriptor_sha256: String,
    build_id: String,
    evidence_sha256: String,
    feature_mask: u64,
    isolated_vm: bool,
    machine_id_sha256: String,
    boot_marker_path: PathBuf,
    boot_marker_sha256: String,
}
#[derive(Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum Admission {
    Candidate(Entry),
    Supported(SupportedEntry),
}
#[derive(Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct SupportedEntry {
    descriptor_sha256: String,
    build_id: String,
    evidence_sha256: String,
    qualification_revision: u64,
    feature_mask: u64,
    approval_revision: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Acceptance {
    acceptance_revision: u64,
    entries: Vec<SupportedEntry>,
}
impl Admission {
    fn identity(&self) -> (&str, &str) {
        match self {
            Self::Candidate(e) => (&e.descriptor_sha256, &e.build_id),
            Self::Supported(e) => (&e.descriptor_sha256, &e.build_id),
        }
    }
    fn qualification(&self) -> Qualification {
        match self {
            Self::Candidate(_) => Qualification::Candidate,
            Self::Supported(_) => Qualification::Supported,
        }
    }
    fn feature_mask(&self) -> u64 {
        match self {
            Self::Candidate(e) => e.feature_mask,
            Self::Supported(e) => e.feature_mask,
        }
    }
    fn verify(&self) -> io::Result<()> {
        match self {
            Self::Candidate(e) => verify_scope(e),
            Self::Supported(e) => verify_acceptance(
                e,
                include_bytes!(concat!(env!("OUT_DIR"), "/hyprland-acceptance.json")),
                env!("SLEDGE_EMBEDDED_ACCEPTANCE_SHA256"),
            ),
        }
    }
}
fn verify_acceptance(entry: &SupportedEntry, bytes: &[u8], pin: &str) -> io::Result<()> {
    if bytes.is_empty() || bytes.len() > 65536 || !hex(pin) || digest(bytes) != pin {
        return Err(denied("missing/mismatched compiled acceptance"));
    }
    let acceptance: Acceptance =
        serde_json::from_slice(bytes).map_err(|_| denied("invalid acceptance schema"))?;
    if acceptance.acceptance_revision != 1
        || acceptance.entries.len() > 16
        || acceptance.entries.iter().filter(|e| *e == entry).count() != 1
    {
        return Err(denied("supported binding not independently accepted"));
    }
    Ok(())
}
/// Opaque result from embedded policy + verified actual metadata + scope guard.
pub struct AdmittedMetadata {
    pub(super) metadata: VerifiedMetadata,
    pub(super) feature_mask: u64,
    qualification: Qualification,
}
impl AdmittedMetadata {
    #[must_use]
    pub const fn qualification(&self) -> Qualification {
        self.qualification
    }
    #[must_use]
    pub const fn feature_mask(&self) -> u64 {
        self.feature_mask
    }
    /// Only independently accepted Supported metadata qualifies host capture.
    /// # Errors
    /// Rejects Candidate; normal builds contain no accepted registration.
    pub fn require_qualified(&self) -> io::Result<()> {
        if self.qualification == Qualification::Supported {
            Ok(())
        } else {
            Err(denied("candidate is not supported host qualification"))
        }
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
        entry.verify()
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
        entry.verify()?;
        Ok(AdmittedMetadata {
            metadata,
            feature_mask: entry.feature_mask(),
            qualification: entry.qualification(),
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
fn select(bytes: &[u8], pin: &str, descriptor: &str, build: &str) -> io::Result<Admission> {
    if bytes.is_empty() || bytes.len() > 65536 || !hex(pin) || digest(bytes) != pin {
        return Err(denied("missing/mismatched compile-pinned catalogue"));
    }
    let policy: Policy =
        serde_json::from_slice(bytes).map_err(|_| denied("invalid catalogue schema"))?;
    if !matches!(policy.policy_revision, 1 | 2) || policy.entries.len() > 16 {
        return Err(denied("unsupported catalogue policy revision/budget"));
    }
    let mut entries = policy
        .entries
        .into_iter()
        .filter(|e| e.identity() == (descriptor, build));
    let entry = entries
        .next()
        .ok_or_else(|| denied("descriptor not in compiled catalogue"))?;
    if entries.next().is_some() {
        return Err(denied("ambiguous catalogue descriptor"));
    }
    match &entry {
        Admission::Candidate(e) => {
            if !e.isolated_vm
                || e.feature_mask != FRESH_DOWN_SEND_KEY
                || !hex(&e.descriptor_sha256)
                || !hex(&e.evidence_sha256)
                || !hex(&e.machine_id_sha256)
                || !hex(&e.boot_marker_sha256)
                || !e.boot_marker_path.is_absolute()
            {
                return Err(denied("unsupported candidate features/scope"));
            }
        }
        Admission::Supported(e) => {
            if policy.policy_revision != 2
                || e.qualification_revision != 1
                || e.feature_mask != FRESH_DOWN_SEND_KEY
                || e.approval_revision == 0
                || !hex(&e.descriptor_sha256)
                || !hex(&e.evidence_sha256)
            {
                return Err(denied("unsupported supported revision/features"));
            }
        }
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
    fn supported_requires_separate_exact_pinned_acceptance() {
        let d = "a".repeat(64);
        let b = format!("sledge-stock-v1:{}", "b".repeat(64));
        let binding = serde_json::json!({"descriptor_sha256":d,"build_id":b,"evidence_sha256":"c".repeat(64),"qualification_revision":1,"feature_mask":1,"approval_revision":9});
        let mut entry = binding.clone();
        entry["status"] = serde_json::json!("supported");
        let policy = serde_json::json!({"policy_revision":2,"entries":[entry]});
        let bytes = serde_json::to_vec(&policy).unwrap();
        let admitted = select(&bytes, &digest(&bytes), &d, &b).unwrap();
        let Admission::Supported(selected) = admitted else {
            panic!("wrong variant")
        };
        assert!(verify_acceptance(&selected, b"", "").is_err());
        let record = serde_json::json!({"acceptance_revision":1,"entries":[binding]});
        let approval = serde_json::to_vec(&record).unwrap();
        assert!(verify_acceptance(&selected, &approval, &digest(&approval)).is_ok());
        assert!(verify_acceptance(&selected, &approval, &"0".repeat(64)).is_err());
        for field in [
            "descriptor_sha256",
            "build_id",
            "evidence_sha256",
            "approval_revision",
            "feature_mask",
            "qualification_revision",
        ] {
            let mut bad = record.clone();
            bad["entries"][0][field] = if matches!(
                field,
                "approval_revision" | "feature_mask" | "qualification_revision"
            ) {
                serde_json::json!(2)
            } else {
                serde_json::json!("f".repeat(64))
            };
            let approval = serde_json::to_vec(&bad).unwrap();
            assert!(
                verify_acceptance(&selected, &approval, &digest(&approval)).is_err(),
                "{field}"
            );
        }
        for (field, value) in [
            ("isolated_vm", serde_json::json!(true)),
            ("feature_mask", serde_json::json!(15)),
            ("qualification_revision", serde_json::json!(2)),
            ("approval_revision", serde_json::json!(0)),
        ] {
            let mut bad = policy.clone();
            bad["entries"][0][field] = value;
            let bytes = serde_json::to_vec(&bad).unwrap();
            assert!(select(&bytes, &digest(&bytes), &d, &b).is_err());
        }
        let mut bad = policy;
        bad["policy_revision"] = serde_json::json!(1);
        let bytes = serde_json::to_vec(&bad).unwrap();
        assert!(select(&bytes, &digest(&bytes), &d, &b).is_err());
    }
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
        bad["policy_revision"] = serde_json::json!(3);
        let bytes = serde_json::to_vec(&bad).unwrap();
        assert!(select(&bytes, &digest(&bytes), &d, &b).is_err());
    }
}
