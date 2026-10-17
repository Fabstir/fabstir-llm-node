//! Policy schema 3 (V3.2), D21: the independent second gate re-derives the
//! authority decision from the RAW documents and the verified evidence. The
//! gate returns early for test entries, so every case here is a REAL entry
//! (`entry_test = false`) with real-shaped GPU claims; the positive cases
//! prove a gate that refused everything could not be green. Each case names
//! what the gate must not take on trust from the loader or step 7.

use super::policy_fixture::{
    address_of, authority_for, bundle, event_log, patched_quote, real_model_id, recording_policy,
    recording_profile, root_key, sign_doc, signer_key, HWMODEL, NOW,
};
use fabstir_llm_node::kbs::cpu::{simulator, TdxEvidence};
use fabstir_llm_node::kbs::eventlog::{replay_and_extract, Replayed};
use fabstir_llm_node::kbs::nras_claims::{GpuFields, GpuOutcome};
use fabstir_llm_node::kbs::routes::release_gate;
use fabstir_llm_node::kbs::verify::{check_policy, Expected, Verified};
use fabstir_llm_node::tee::host_profile::{AuthorityBundle, SignedAuthorityManifest};
use fabstir_llm_node::tee::types::Policy;

const PK: [u8; 33] = [0x02; 33];
const NONCE: [u8; 32] = [0x77; 32];

fn policy() -> Policy {
    recording_policy(real_model_id(1), 1)
}

fn cpu() -> TdxEvidence {
    simulator(&patched_quote(&PK, &NONCE)).unwrap()
}

fn events() -> Replayed {
    replay_and_extract(&event_log()).unwrap()
}

fn gpu() -> GpuOutcome {
    GpuOutcome::Real(GpuFields {
        nonce: NONCE,
        hwmodel: HWMODEL.into(),
        secure_boot: true,
        debug_disabled: true,
        driver_version: "550.90.07".into(),
        vbios_version: "96.00.74.00.1a".into(),
    })
}

/// Step 7 on the pristine documents: the record the gate is handed.
fn step7(p: &Policy, b: &AuthorityBundle) -> Verified {
    let a = authority_for(p, b, NOW);
    let exp = Expected {
        model_id: real_model_id(1),
        pk_att: PK,
        nonce: NONCE,
    };
    check_policy(p, &a, &exp, &cpu(), &events(), &gpu(), false, NOW).unwrap()
}

fn gate(
    p: &Policy,
    b: &AuthorityBundle,
    v: &Verified,
    cpu: &TdxEvidence,
    r: &Replayed,
    now: u64,
) -> Result<(), String> {
    release_gate(&b.manifest, &b.set, now, cpu, r, &gpu(), p, v, false).map_err(|e| e.detail)
}

#[test]
fn release_gate_passes_a_good_real_entry_and_a_cosigned_transition_set() {
    // Mutations: refuse a non-current co-signature in the gate; refuse always.
    let p = policy();
    let b = bundle(1, 1, vec![recording_profile()]);
    let v = step7(&p, &b);
    gate(&p, &b, &v, &cpu(), &events(), NOW).expect("a good real entry releases");
    // The profile window is inclusive at both ends in the gate too.
    let prof = &b.set.set.profiles[0];
    gate(&p, &b, &v, &cpu(), &events(), prof.not_before).expect("now == profile.not_before");
    gate(&p, &b, &v, &cpu(), &events(), prof.expiry).expect("now == profile.expiry");
    // A non-candidate copy of the matching profile (expired) is not in the
    // gate's re-derived matched set either, so it does not cause a false
    // disagreement.
    let mut extra = b.clone();
    let mut old = extra.set.set.profiles[0].clone();
    (old.id, old.not_before, old.expiry) = ("aa/expired".into(), NOW - 10, NOW - 1);
    extra.set.set.profiles.push(old);
    extra.set.signatures = vec![sign_doc(&extra.set.set, &signer_key(0))];
    let ve = step7(&p, &extra);
    gate(&p, &extra, &ve, &cpu(), &events(), NOW).expect("an expired copy changes nothing");
    // ...and the manifest window: exactly [NOW, NOW] passes at NOW.
    let mut tight = b.clone();
    (
        tight.manifest.manifest.not_before,
        tight.manifest.manifest.expiry,
    ) = (NOW, NOW);
    tight.manifest.signature = sign_doc(&tight.manifest.manifest, &root_key());
    let vt = step7(&p, &tight);
    gate(&p, &tight, &vt, &cpu(), &events(), NOW).expect("now == manifest bounds");
    // The transition set co-signed by an incoming (not yet current) signer
    // passes under manifest v1 and, after the bump, under v2.
    let mut t = b.clone();
    t.set.signatures.push(sign_doc(&t.set.set, &signer_key(1)));
    let v = step7(&p, &t);
    gate(&p, &t, &v, &cpu(), &events(), NOW).expect("under v1");
    let mut m2 = t.manifest.manifest.clone();
    m2.manifest_version = 2;
    m2.signers.push(address_of(&signer_key(1)));
    m2.threshold = 2;
    t.manifest = SignedAuthorityManifest {
        signature: sign_doc(&m2, &root_key()),
        manifest: m2,
    };
    let v = step7(&p, &t);
    gate(&p, &t, &v, &cpu(), &events(), NOW).expect("under v2");
}

#[test]
fn release_gate_authenticates_the_manifest_rechecks_minimums_recounts_signers_and_refuses_headline_not_in_set_out_of_window_unpinned_mismatching_or_rtmr3_mismatch(
) {
    // One case each. Mutation per case: the gate trusts the loader / step 7
    // for that item → the case releases → red.
    let p = policy();
    let good = bundle(1, 1, vec![recording_profile()]);
    let v = step7(&p, &good);
    let other = signer_key(7);
    type Case = (
        &'static str,
        Box<
            dyn Fn(
                &mut Policy,
                &mut AuthorityBundle,
                &mut Verified,
                &mut TdxEvidence,
                &mut Replayed,
                &mut u64,
            ),
        >,
    );
    let cases: Vec<Case> = vec![
        (
            "authority roots disagree",
            Box::new(|_, b, _, _, _, _| {
                // A manifest signed by another key (the loader would refuse it).
                b.manifest.signature = sign_doc(&b.manifest.manifest, &signer_key(7));
            }),
        ),
        (
            "authority roots disagree",
            Box::new(move |p, _, _, _, _, _| p.host_authority.root = address_of(&other)),
        ),
        (
            // Only manifest.root differs: the policy root signed a manifest
            // naming another root.
            "authority roots disagree",
            Box::new(|_, b, _, _, _, _| {
                b.manifest.manifest.root = address_of(&signer_key(7));
                b.manifest.signature = sign_doc(&b.manifest.manifest, &root_key());
            }),
        ),
        (
            // Only set.root differs: a current signer signed a set of another root.
            "authority roots disagree",
            Box::new(|_, b, _, _, _, _| {
                b.set.set.root = address_of(&signer_key(7));
                b.set.signatures = vec![sign_doc(&b.set.set, &signer_key(0))];
            }),
        ),
        (
            "manifest v1 outside its window",
            Box::new(|_, b, _, _, _, now| {
                b.manifest.manifest.expiry = *now - 1;
                b.manifest.signature = sign_doc(&b.manifest.manifest, &root_key());
            }),
        ),
        (
            "manifest v1 outside its window",
            Box::new(|_, b, _, _, _, now| {
                b.manifest.manifest.not_before = *now + 1;
                b.manifest.signature = sign_doc(&b.manifest.manifest, &root_key());
            }),
        ),
        (
            "below min_manifest_version",
            Box::new(|p, _, _, _, _, _| p.host_authority.min_manifest_version = 2),
        ),
        (
            "below min_threshold",
            Box::new(|p, _, _, _, _, _| p.host_authority.min_threshold = 2),
        ),
        (
            "below min_profile_epoch",
            Box::new(|p, _, _, _, _, _| p.host_authority.min_profile_epoch = 2),
        ),
        (
            "0 distinct current signer",
            Box::new(|_, b, _, _, _, _| {
                b.set.signatures = vec![sign_doc(&b.set.set, &signer_key(3))];
            }),
        ),
        (
            // Threshold 2 with ONE signer's signature twice: counts once.
            "1 distinct current signer",
            Box::new(|_, b, _, _, _, _| {
                let m = &mut b.manifest.manifest;
                m.signers.push(address_of(&signer_key(1)));
                m.threshold = 2;
                b.manifest.signature = sign_doc(&b.manifest.manifest, &root_key());
                let sig = sign_doc(&b.set.set, &signer_key(0));
                b.set.signatures = vec![sig.clone(), sig];
            }),
        ),
        (
            "headline profile ghost is not in epoch 1",
            Box::new(|_, _, v, _, _, _| v.profile.headline = "ghost".into()),
        ),
        (
            "profile recording/simulator outside its window",
            Box::new(|_, b, _, _, _, now| *now = b.set.set.profiles[0].expiry + 1),
        ),
        (
            "profile recording/simulator outside its window",
            Box::new(|_, b, _, _, _, now| *now = b.set.set.profiles[0].not_before - 1),
        ),
        (
            "class test-class not allowed",
            Box::new(|p, _, _, _, _, _| p.host_authority.allowed_classes = vec!["other".into()]),
        ),
        (
            "not pinned as the policy requires",
            Box::new(|p, b, _, _, _, _| {
                p.cvm.require_key_provider_pinned = true;
                b.set.set.profiles[0].cvm.app_id = None;
                b.set.set.profiles[0].cvm.key_provider = None;
                b.set.signatures = vec![sign_doc(&b.set.set, &signer_key(0))];
            }),
        ),
        (
            // The app-id half: both pins required, only the app id missing.
            "not pinned as the policy requires",
            Box::new(|p, b, _, _, _, _| {
                (
                    p.cvm.require_app_id_pinned,
                    p.cvm.require_key_provider_pinned,
                ) = (true, true);
                b.set.set.profiles[0].cvm.app_id = None;
                b.set.signatures = vec![sign_doc(&b.set.set, &signer_key(0))];
            }),
        ),
        (
            "below floor 999.0 of profile recording/simulator",
            Box::new(|_, b, _, _, _, _| {
                b.set.set.profiles[0].gpu.min_driver_version = Some("999.0".into());
                b.set.signatures = vec![sign_doc(&b.set.set, &signer_key(0))];
            }),
        ),
        (
            "below floor 999.0 of the policy",
            Box::new(|p, _, _, _, _, _| p.gpu.min_vbios_version = Some("999.0".into())),
        ),
        ("on mrtd", Box::new(|_, _, _, c, _, _| c.mr_td[0] ^= 1)),
        ("on rtmr0", Box::new(|_, _, _, c, _, _| c.rt_mr0[0] ^= 1)),
        ("on rtmr1", Box::new(|_, _, _, c, _, _| c.rt_mr1[0] ^= 1)),
        ("on rtmr2", Box::new(|_, _, _, c, _, _| c.rt_mr2[0] ^= 1)),
        (
            "on os image",
            Box::new(|_, _, _, _, r, _| r.os_image_hash[0] ^= 1),
        ),
        (
            "on compose hash",
            Box::new(|_, _, _, _, r, _| r.compose_hash[0] ^= 1),
        ),
        ("on app id", Box::new(|_, _, _, _, r, _| r.app_id = None)),
        (
            "on key provider",
            Box::new(|_, _, _, _, r, _| r.key_provider.as_mut().unwrap()[0] ^= 1),
        ),
        (
            "on hwmodel",
            Box::new(|_, b, _, _, _, _| {
                b.set.set.profiles[0].gpu.hwmodels = vec!["NOT-THIS-GPU".into()];
                b.set.signatures = vec![sign_doc(&b.set.set, &signer_key(0))];
            }),
        ),
        (
            "on rtmr3 replay",
            Box::new(|_, _, _, _, r, _| r.rtmr3[0] ^= 1),
        ),
        (
            // Step 7 recorded no match list at all.
            "recorded matched profiles []",
            Box::new(|_, _, v, _, _, _| v.profile.all.clear()),
        ),
        (
            // Step 7 recorded a profile that is not in the set.
            "recorded matched profiles [\"recording/simulator\", \"ghost\"]",
            Box::new(|_, _, v, _, _, _| v.profile.all.push("ghost".into())),
        ),
        (
            // A step-7 bug DROPPED a second matching profile (with a floor the
            // GPU fails): the gate's own matched set disagrees, so neither the
            // match nor its floor can be lost.
            "the documents say [\"recording/simulator\", \"zz",
            Box::new(|_, b, _, _, _, _| {
                let mut zz = b.set.set.profiles[0].clone();
                zz.id = "zz/second".into();
                zz.gpu.min_driver_version = Some("999.0".into());
                b.set.set.profiles.push(zz);
                b.set.signatures = vec![sign_doc(&b.set.set, &signer_key(0))];
            }),
        ),
        (
            // Two matched profiles, both recorded; the SECOND one's floor fails.
            "below floor 999.0 of profile zz/second",
            Box::new(|_, b, v, _, _, _| {
                let mut zz = b.set.set.profiles[0].clone();
                zz.id = "zz/second".into();
                zz.gpu.min_driver_version = Some("999.0".into());
                b.set.set.profiles.push(zz);
                b.set.signatures = vec![sign_doc(&b.set.set, &signer_key(0))];
                v.profile.all.push("zz/second".into());
            }),
        ),
        (
            "the policy step recorded epoch 9",
            Box::new(|_, _, v, _, _, _| v.profile.epoch = 9),
        ),
        (
            "recorded epoch 1 manifest v9",
            Box::new(|_, _, v, _, _, _| v.profile.manifest_version = 9),
        ),
        (
            "manifest v1 signers [\"0x9999",
            Box::new(|_, _, v, _, _, _| {
                v.profile.counted_signers = vec![format!("0x{}", "9".repeat(40))]
            }),
        ),
    ];
    // Every case is judged and every wrong one reported (not just the first).
    let mut wrong = Vec::new();
    for (needle, mutate) in cases {
        let (mut p, mut b, mut v, mut c, mut r, mut now) =
            (p.clone(), good.clone(), v.clone(), cpu(), events(), NOW);
        mutate(&mut p, &mut b, &mut v, &mut c, &mut r, &mut now);
        match gate(&p, &b, &v, &c, &r, now) {
            Err(m) if m.contains(needle) => {}
            other => wrong.push(format!("CASE[{needle}] {other:?}")),
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
    // The hwmodel must be in the profile's list AND the policy's.
    let mut p2 = p.clone();
    p2.gpu.allowed_hwmodels = vec!["OTHER".into()];
    let e = release_gate(
        &good.manifest,
        &good.set,
        NOW,
        &cpu(),
        &events(),
        &gpu(),
        &p2,
        &v,
        false,
    )
    .unwrap_err();
    assert!(e.detail.contains("on hwmodel"), "{e}");
}
