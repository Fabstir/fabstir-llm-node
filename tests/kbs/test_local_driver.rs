//! The driver for `scripts/kbs-local-run.sh` (policy schema 3, V3.6), not a
//! gate: both tests are `#[ignore]` and run only from the script.
//!
//! `prepare` writes the UNSIGNED documents (policy, manifest, two profiles of
//! the simulator recording's identity) that the script then signs and
//! assembles with the real tooling. `transcript` talks to the script's live
//! `fabstir-kbs serve` (simulator CPU evidence, canned GPU evidence, a test
//! keyring) over loopback HTTP, swapping the served bundle between steps:
//! epoch 1 releases; epoch 2 (the profile removed) refuses with the archive
//! hint; epoch 1 re-served is refused below the high-water mark; epoch 3 (the
//! profile back) releases. The policy file, and so `policy_hash`, never
//! changes: a host change is an epoch, never a reseal.
//!
//! Inputs are `LOCAL_KBS_*`, never `KBS_*` (the broker refuses an unknown
//! `KBS_*` name): `LOCAL_KBS_DIR` (the run directory) for both;
//! `LOCAL_KBS_ROOT`, `LOCAL_KBS_SIGNER` and `LOCAL_KBS_MODEL` for `prepare`;
//! `LOCAL_KBS_URL` for `transcript`.

use super::fixtures::fixture;
use super::policy_fixture::{event_log, patched_quote, recording_policy, recording_profile};
use fabstir_llm_node::kbs::authority::root_key;
use fabstir_llm_node::tee::host_profile::{AuthorityManifest, MANIFEST_DOC};
use fabstir_llm_node::tee::kbs_http::{
    BrokerInfo, ChallengeRequest, ChallengeResponse, ErrorBody, EvidenceWire, RequestKeyRequest,
    RequestKeyResponse,
};
use fabstir_llm_node::tee::keywrap::{generate_ephemeral_keypair, unwrap_key};
use fabstir_llm_node::tee::policy::SignedModelPolicy;
use fabstir_llm_node::tee::types::{sha256_32, Evidence};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The class every local profile carries and the local policy allows.
const CLASS: &str = "local-test";
/// The simulator recording's identity, valid now.
const LIVE: &str = "local/simulator";
/// The same identity, its window ended yesterday.
const EXPIRED: &str = "local/simulator-expired";

fn var(name: &str) -> String {
    std::env::var(name)
        .unwrap_or_else(|_| panic!("{name} is not set (run scripts/kbs-local-run.sh)"))
}

fn real_now() -> u64 {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
    now.unwrap().as_secs()
}

fn write_json<T: serde::Serialize>(path: &Path, v: &T) {
    std::fs::write(path, serde_json::to_vec_pretty(v).unwrap())
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
}

fn say(line: impl AsRef<str>) {
    println!("transcript| {}", line.as_ref());
}

#[test]
#[ignore = "driver for scripts/kbs-local-run.sh, not a gate"]
fn prepare() {
    let dir = PathBuf::from(var("LOCAL_KBS_DIR"));
    let root = var("LOCAL_KBS_ROOT");
    let model_id: [u8; 32] = hex::decode(var("LOCAL_KBS_MODEL"))
        .unwrap()
        .try_into()
        .expect("LOCAL_KBS_MODEL is 32 bytes of hex");
    let now = real_now();

    // The recording policy (schema 3, no pins, the recording GPU, a window
    // around now) under this run's root and class.
    let mut policy = recording_policy(model_id, 1);
    policy.host_authority.root = root.clone();
    policy.host_authority.allowed_classes = vec![CLASS.into()];
    write_json(&dir.join("policy.json"), &policy);

    let manifest = AuthorityManifest {
        doc: MANIFEST_DOC.into(),
        schema_version: 1,
        root,
        manifest_version: 1,
        signers: vec![var("LOCAL_KBS_SIGNER")],
        threshold: 1,
        not_before: now - 3600,
        expiry: now + 90 * 86_400,
    };
    write_json(&dir.join("manifest.json"), &manifest);

    // Registers off the recording's quote, compose/os image/app id/key
    // provider off its event log: the fixture's derivation, re-windowed.
    let mut live = recording_profile();
    live.id = LIVE.into();
    live.class = CLASS.into();
    live.not_before = now - 86_400;
    live.expiry = now + 30 * 86_400;
    live.evidence = "dstack 0.5.9 simulator recording; local run".into();
    let mut expired = live.clone();
    expired.id = EXPIRED.into();
    expired.not_before = now - 60 * 86_400;
    expired.expiry = now - 86_400;
    write_json(&dir.join("profile-live.json"), &live);
    write_json(&dir.join("profile-expired.json"), &expired);
    say(format!(
        "prepared policy, manifest, profiles {LIVE} and {EXPIRED}"
    ));
}

/// The script's run directory, as the driver sees it.
struct Run {
    dir: PathBuf,
    data: PathBuf,
    url: String,
    http: reqwest::Client,
    model_id: [u8; 32],
    root: String,
}

enum Outcome {
    Released {
        test_release: bool,
        dek_matches: bool,
        decision: String,
    },
    Refused {
        at: &'static str,
        status: u16,
        kind: String,
        detail: String,
    },
}

impl Run {
    fn from_env() -> Self {
        let dir = PathBuf::from(var("LOCAL_KBS_DIR"));
        let policy: SignedModelPolicy =
            serde_json::from_slice(&std::fs::read(dir.join("policy.signed.json")).unwrap())
                .unwrap();
        Self {
            data: dir.join("data"),
            dir,
            url: var("LOCAL_KBS_URL"),
            http: reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(180))
                .build()
                .unwrap(),
            model_id: policy.policy.model_id,
            root: policy.policy.host_authority.root,
        }
    }

    /// The SERVED policy file: its bytes' sha256 and its `policy_hash`.
    fn served_policy(&self) -> (String, String) {
        let path = self
            .data
            .join("public/policies")
            .join(format!("{}.json", hex::encode(self.model_id)));
        let bytes = std::fs::read(&path).unwrap();
        let signed: SignedModelPolicy = serde_json::from_slice(&bytes).unwrap();
        (
            hex::encode(sha256_32(&bytes)),
            hex::encode(signed.policy_hash().unwrap()),
        )
    }

    /// Serve `bundle-e{epoch}.json` as the root's bundle (written aside, then
    /// renamed: the broker never reads a half-written file).
    fn serve(&self, epoch: u64) {
        let dir = self.data.join("public/authority");
        let name = format!("{}.json", root_key(&self.root));
        let tmp = dir.join(format!(".{name}.tmp"));
        std::fs::copy(self.dir.join(format!("bundle-e{epoch}.json")), &tmp).unwrap();
        std::fs::rename(&tmp, dir.join(name)).unwrap();
    }

    /// The newest capture's `decision.txt` in `ring`.
    fn newest_decision(&self, ring: &str) -> String {
        let ring = self.data.join("capture").join(ring);
        let newest = std::fs::read_dir(&ring)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| !n.starts_with('.'))
            .max()
            .expect("a capture");
        std::fs::read_to_string(ring.join(newest).join("decision.txt")).unwrap()
    }

    /// What a node does: challenge, evidence bound to the issued nonce (the
    /// simulator quote patched as the harness does, NVIDIA's canned GPU
    /// evidence carrying the nonce), request_key, unwrap.
    async fn attempt(&self) -> Outcome {
        let (sk, pk) = generate_ephemeral_keypair();
        let r = self
            .http
            .post(format!("{}/v1/kbs/challenge", self.url))
            .json(&ChallengeRequest {
                model_id: hex::encode(self.model_id),
                pk_att: hex::encode(&pk),
            })
            .send()
            .await
            .unwrap();
        if !r.status().is_success() {
            return refused("challenge", r).await;
        }
        let ch: ChallengeResponse = r.json().await.unwrap();
        let nonce: [u8; 32] = hex::decode(&ch.nonce).unwrap().try_into().unwrap();

        let mut gpu: serde_json::Value =
            serde_json::from_slice(&fixture("a17-canned-payload.json")).unwrap();
        gpu["nonce"] = hex::encode(nonce).into();
        let ev = Evidence {
            gpu_report: serde_json::to_vec(&gpu).unwrap(),
            cpu_quote: patched_quote(pk.as_slice().try_into().unwrap(), &nonce),
            event_log: event_log(),
            vm_config: b"{}".to_vec(),
            image_measurement: [0; 48],
            pk_att: pk.clone(),
            nonce,
        };
        let r = self
            .http
            .post(format!("{}/v1/kbs/request_key", self.url))
            .json(&RequestKeyRequest {
                model_id: hex::encode(self.model_id),
                evidence: EvidenceWire::encode(&ev).unwrap(),
            })
            .send()
            .await
            .unwrap();
        if !r.status().is_success() {
            return refused("request_key", r).await;
        }
        let resp: RequestKeyResponse = r.json().await.unwrap();
        let dek = unwrap_key(&resp.wrapped_key.decode().unwrap(), &sk).unwrap();
        let want = std::fs::read_to_string(self.dir.join("keys/dek.hex")).unwrap();
        Outcome::Released {
            test_release: resp.test_release,
            dek_matches: hex::encode(dek) == want.trim(),
            decision: self.newest_decision("verified"),
        }
    }
}

async fn refused(at: &'static str, r: reqwest::Response) -> Outcome {
    let status = r.status().as_u16();
    let body: ErrorBody = r.json().await.unwrap();
    Outcome::Refused {
        at,
        status,
        kind: body.error.kind,
        detail: body.error.detail,
    }
}

/// Print one step and return the released decision's `policy_hash`, if any.
fn report(step: &str, o: &Outcome) -> Option<String> {
    say(format!("== {step}"));
    match o {
        Outcome::Released {
            test_release,
            dek_matches,
            decision,
        } => {
            say(format!(
                "   released: test_release={test_release} dek_matches_keyring={dek_matches}"
            ));
            // The broker's own `released` line (decision.txt's second line).
            let line = decision.lines().nth(1).unwrap_or_default();
            const SHOWN: &str = "policy_version policy_hash profile class epoch manifest \
                                 set_digest manifest_digest signers tcb cc_mode";
            for field in line.split_whitespace().filter(|f| {
                let key = f.split('=').next().unwrap_or_default();
                SHOWN.split_whitespace().any(|k| k == key)
            }) {
                say(format!("   broker: {field}"));
            }
            line.split_whitespace()
                .find_map(|f| f.strip_prefix("policy_hash="))
                .map(str::to_string)
        }
        Outcome::Refused {
            at,
            status,
            kind,
            detail,
        } => {
            say(format!("   refused at {at}: {status} {kind}"));
            for row in detail.split("; ") {
                say(format!("   | {row}"));
            }
            None
        }
    }
}

fn assert_released(o: &Outcome, epoch: u64) {
    let Outcome::Released {
        test_release,
        dek_matches,
        decision,
    } = o
    else {
        panic!("expected a release at epoch {epoch}");
    };
    assert!(*test_release && *dek_matches, "{decision}");
    for want in [format!("profile={LIVE}"), format!("epoch={epoch}")] {
        assert!(decision.contains(&want), "{want} not in {decision}");
    }
}

fn assert_refused(o: &Outcome, at: &str, needles: &[&str]) {
    let Outcome::Refused {
        at: got, detail, ..
    } = o
    else {
        panic!("expected a refusal at {at}");
    };
    assert_eq!(*got, at, "{detail}");
    for n in needles {
        assert!(detail.contains(n), "{n:?} not in {detail}");
    }
}

#[tokio::test]
#[ignore = "driver for scripts/kbs-local-run.sh, not a gate"]
async fn transcript() {
    let run = Run::from_env();
    let info: BrokerInfo = run
        .http
        .get(format!("{}/v1/kbs/info", run.url))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    say(format!(
        "broker {} keyring={} cpu_evidence={} gpu_evidence={}",
        info.version, info.keyring, info.cpu_evidence, info.gpu_evidence
    ));
    assert_eq!(
        (&*info.keyring, &*info.cpu_evidence, &*info.gpu_evidence),
        ("test", "simulator", "canned")
    );
    let (file0, hash0) = run.served_policy();
    say(format!(
        "policy file sha256={file0} policy_hash={hash0} (model {})",
        hex::encode(run.model_id)
    ));
    let mut hashes = Vec::new();

    run.serve(1);
    let o = run.attempt().await;
    hashes.extend(report(
        &format!("1. epoch 1 served ({LIVE} + {EXPIRED})"),
        &o,
    ));
    assert_released(&o, 1);

    run.serve(2);
    let o = run.attempt().await;
    report(&format!("2. epoch 2 served ({LIVE} removed)"), &o);
    assert_refused(
        &o,
        "request_key",
        &[
            "candidates=0 of 1 (expired=1, class_filtered=0, unpinned=0) in epoch 2",
            &format!("matches {EXPIRED} but expired at"),
            &format!("matches {LIVE}, present in the archived bundle"),
            "but absent from epoch 2",
        ],
    );

    run.serve(1);
    let o = run.attempt().await;
    report("3. epoch 1 re-served (a rollback)", &o);
    assert_refused(&o, "challenge", &["epoch 1 is below the high-water mark 2"]);

    run.serve(3);
    let o = run.attempt().await;
    hashes.extend(report(&format!("4. epoch 3 served ({LIVE} back)"), &o));
    assert_released(&o, 3);

    // No reseal: the served file is byte-identical and both releases name its hash.
    assert_eq!(
        run.served_policy(),
        (file0, hash0.clone()),
        "policy changed"
    );
    assert_eq!(
        hashes,
        [hash0.clone(), hash0.clone()],
        "released policy_hash"
    );
    say(format!(
        "policy file unchanged across all four steps; both releases under policy_hash={hash0}"
    ));
}
