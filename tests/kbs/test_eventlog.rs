//! Design §9 / A-10 on REAL bytes: the vendored dstack 0.5.9 recording (wire-form log +
//! raw quote). Extraction tests call `extract` directly (the replay row would otherwise
//! refuse first and mask the mutation).

use super::fixtures::fixture;
use fabstir_llm_node::kbs::eventlog::{
    extract, parse, replay, replay_and_extract, runtime_digest, select_runtime, RuntimeEvent,
    EV_BOOT_MR_DONE, EV_COMPOSE_HASH, EV_OS_IMAGE_HASH, EV_SYSTEM_READY, RUNTIME_EVENT_TYPE,
};

const RT_MR3_OFFSET: usize = 520;

fn recording() -> (Vec<u8>, Vec<u8>) {
    (
        fixture("dstack-0.5.9-simulator-eventlog.json"),
        fixture("dstack-0.5.9-simulator-quote.bin"),
    )
}

fn quote_rtmr3(quote: &[u8]) -> [u8; 48] {
    quote[RT_MR3_OFFSET..RT_MR3_OFFSET + 48].try_into().unwrap()
}

fn events() -> Vec<RuntimeEvent> {
    let (log, _) = recording();
    select_runtime(&parse(&log).unwrap()).unwrap()
}

#[test]
fn recording_replays_to_its_quotes_rt_mr3() {
    let (log, quote) = recording();
    let r = replay_and_extract(&log).unwrap();
    assert_eq!(r.rtmr3, quote_rtmr3(&quote));
    assert_eq!(hex::encode(r.compose_hash).len(), 64);
    assert_eq!(r.app_id.as_ref().map(|v| v.len()), Some(20));
    assert_eq!(r.key_provider.as_ref().map(|v| v.len()), Some(204));
    let kp = String::from_utf8(r.key_provider.unwrap()).unwrap();
    assert!(kp.starts_with(r#"{"name":"kms","id":""#), "{kp}");
}

#[test]
fn one_flipped_payload_byte_breaks_the_chain() {
    let (_, quote) = recording();
    let mut ev = events();
    let i = ev.iter().position(|e| e.event == EV_COMPOSE_HASH).unwrap();
    ev[i].payload[0] ^= 1;
    ev[i].served_digest = None; // the live form; only the chain can catch it
    assert_ne!(replay(&ev).unwrap(), quote_rtmr3(&quote));
}

#[test]
fn live_stripped_form_still_replays() {
    // Derive the live form: runtime entries lose their digest, every other entry
    // loses its payload. All 33 entries stay.
    let (log, quote) = recording();
    let mut v: Vec<serde_json::Value> = serde_json::from_slice(&log).unwrap();
    assert_eq!(v.len(), 33);
    for e in v.iter_mut() {
        if e["event_type"].as_u64().unwrap() == RUNTIME_EVENT_TYPE as u64 {
            e["digest"] = serde_json::Value::String(String::new());
        } else {
            e["event_payload"] = serde_json::Value::String(String::new());
        }
    }
    let stripped = serde_json::to_vec(&v).unwrap();
    let r = replay_and_extract(&stripped).unwrap();
    assert_eq!(r.rtmr3, quote_rtmr3(&quote));
    assert!(select_runtime(&parse(&stripped).unwrap())
        .unwrap()
        .iter()
        .all(|e| e.served_digest.is_none()));
}

#[test]
fn a_present_wrong_digest_is_refused() {
    let mut ev = events();
    let d = ev[1].served_digest.as_mut().unwrap();
    d[0] ^= 1;
    let e = replay(&ev).unwrap_err();
    assert!(e.0.contains("served digest does not match"), "{e}");
    // a correct present digest is accepted (the recording's own)
    assert_eq!(
        ev[0].served_digest.unwrap(),
        runtime_digest(&ev[0].event, &ev[0].payload)
    );
}

#[test]
fn foreign_event_type_or_imr_refuses_with_the_row_named() {
    let (log, _) = recording();
    let mut v: Vec<serde_json::Value> = serde_json::from_slice(&log).unwrap();
    // an imr==3 entry with another type
    let mut a = v.clone();
    let i = a.iter().position(|e| e["imr"] == 3).unwrap();
    a[i]["event_type"] = serde_json::Value::from(0x8000_000Bu32);
    let e = replay_and_extract(&serde_json::to_vec(&a).unwrap()).unwrap_err();
    assert!(e.0.contains("foreign event type/IMR"), "{e}");
    // a runtime-typed entry in IMR2
    let j = v.iter().position(|e| e["imr"] == 2).unwrap();
    v[j]["event_type"] = serde_json::Value::from(RUNTIME_EVENT_TYPE);
    let e = replay_and_extract(&serde_json::to_vec(&v).unwrap()).unwrap_err();
    assert!(e.0.contains("foreign event type/IMR"), "{e}");
}

fn rt(event: &str, payload: &[u8]) -> RuntimeEvent {
    RuntimeEvent {
        event: event.into(),
        payload: payload.to_vec(),
        served_digest: None,
    }
}

#[test]
fn duplicate_pinned_name_before_system_ready_refuses() {
    let mut ev = events();
    let i = ev.iter().position(|e| e.event == EV_COMPOSE_HASH).unwrap();
    let dup = ev[i].clone();
    ev.insert(i + 1, dup);
    let e = extract(&ev, [0u8; 48]).unwrap_err();
    assert!(e.0.contains("duplicate compose-hash"), "{e}");
}

#[test]
fn first_occurrence_wins_and_post_system_ready_events_are_never_extracted() {
    // The A1 attack shape: the genuine compose-hash is another value, a second
    // compose-hash equal to the pin is appended after system-ready.
    let mut ev = events();
    let i = ev.iter().position(|e| e.event == EV_COMPOSE_HASH).unwrap();
    let pin = ev[i].payload.clone();
    ev[i].payload = vec![0xEE; 32];
    ev.push(rt(EV_COMPOSE_HASH, &pin));
    let r = extract(&ev, [0u8; 48]).unwrap();
    assert_eq!(
        r.compose_hash, [0xEE; 32],
        "a last-wins map would have returned the pin"
    );
    assert_ne!(r.compose_hash.to_vec(), pin);
}

#[test]
fn missing_system_ready_boot_mr_done_compose_or_os_image_refuse() {
    let base = events();
    let without = |name: &str| -> Vec<RuntimeEvent> {
        base.iter().filter(|e| e.event != name).cloned().collect()
    };
    assert!(extract(&without(EV_SYSTEM_READY), [0u8; 48])
        .unwrap_err()
        .0
        .contains("system-ready event absent"));
    assert!(extract(&without(EV_BOOT_MR_DONE), [0u8; 48])
        .unwrap_err()
        .0
        .contains("boot-mr-done event absent"));
    assert!(extract(&without(EV_COMPOSE_HASH), [0u8; 48])
        .unwrap_err()
        .0
        .contains("compose-hash event absent"));
    assert!(extract(&without(EV_OS_IMAGE_HASH), [0u8; 48])
        .unwrap_err()
        .0
        .contains("os image"));
    // a 31-byte compose-hash payload
    let mut ev = base.clone();
    let i = ev.iter().position(|e| e.event == EV_COMPOSE_HASH).unwrap();
    ev[i].payload.pop();
    assert!(extract(&ev, [0u8; 48]).unwrap_err().0.contains("want 32"));
}

#[test]
fn key_provider_is_the_raw_payload_not_its_sha256() {
    use sha2::Digest;
    let r = replay_and_extract(&recording().0).unwrap();
    let raw = r.key_provider.unwrap();
    let sha: [u8; 32] = sha2::Sha256::digest(&raw).into();
    assert_ne!(raw.len(), 32);
    assert_ne!(raw.as_slice(), &sha[..]);
}

#[test]
fn digest_formula_is_little_endian_type_colon_event_colon_payload() {
    // hand-computed for the empty system-ready event
    use sha2::Digest;
    let d = runtime_digest(EV_SYSTEM_READY, b"");
    let mut h = sha2::Sha384::new();
    h.update([0x01, 0x00, 0x00, 0x08]);
    h.update(b":system-ready:");
    let want: [u8; 48] = h.finalize().into();
    assert_eq!(d, want);
}

#[test]
fn parse_refuses_non_json_and_non_hex() {
    assert!(parse(b"not json").is_err());
    let bad = br#"[{"imr":3,"event_type":134217729,"digest":"","event":"x","event_payload":"zz"}]"#;
    let e = select_runtime(&parse(bad).unwrap()).unwrap_err();
    assert!(e.0.contains("not hex"), "{e}");
}
