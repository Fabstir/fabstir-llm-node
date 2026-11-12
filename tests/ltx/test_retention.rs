// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Retention of delivered results: the setting, the request the node sends the bridge, and the guard on jobs in
//! flight. Each test names the mutation that turns it red.
use axum::{extract::State, http::StatusCode, routing::post, Json, Router};
use fabstir_llm_node::ltx::retention::{
    days_from_lookup, hold, in_flight_entries, prune_once, request_body, Counts,
};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

fn days(v: Option<&str>) -> Result<Option<u64>, String> {
    days_from_lookup(|k| {
        assert_eq!(k, "LTX_RESULT_RETENTION_DAYS");
        v.map(str::to_string)
    })
}

#[test]
fn the_period_defaults_to_thirty_days_and_zero_disables() {
    // another default -> red; 0 read as "delete everything" -> red; a typo accepted as the default -> red
    assert_eq!(days(None), Ok(Some(30)));
    assert_eq!(days(Some("  ")), Ok(Some(30)));
    assert_eq!(days(Some("0")), Ok(None));
    assert_eq!(days(Some("45")), Ok(Some(45)));
    for bad in ["thirty", "-1", "1.5", "99999"] {
        let e = days(Some(bad)).unwrap_err();
        assert!(e.contains("LTX_RESULT_RETENTION_DAYS"), "{bad}: {e}");
    }
}

#[derive(Clone, Default)]
struct Bridge {
    bodies: Arc<Mutex<Vec<Value>>>,
    answer: Arc<Mutex<Option<(u16, Value)>>>,
}

async fn prune(State(b): State<Bridge>, Json(body): Json<Value>) -> (StatusCode, Json<Value>) {
    b.bodies.lock().unwrap().push(body);
    let (status, answer) = b.answer.lock().unwrap().clone().unwrap_or((
        200,
        json!({"scanned": 7, "deleted": 2, "files": 5, "released": 5, "failed": 1, "skipped": 3, "more": true}),
    ));
    (StatusCode::from_u16(status).unwrap(), Json(answer))
}

async fn bridge() -> (Bridge, String) {
    let b = Bridge::default();
    let app = Router::new()
        .route("/s5/prune", post(prune))
        .with_state(b.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (b, url)
}

#[tokio::test]
async fn a_run_asks_the_bridge_for_the_fixed_tree_the_period_and_spares_jobs_in_flight() {
    // the in-flight names left out -> the exclude assertion -> red; days sent as days, or another tree -> red
    let (b, url) = bridge().await;
    let http = reqwest::Client::new();
    let working = hold("812-retention-test-a");
    let twice = hold("812-retention-test-a");
    let counts = prune_once(&http, &format!("{url}/"), 30).await.unwrap();
    assert_eq!(
        counts,
        Counts {
            scanned: 7,
            deleted: 2,
            files: 5,
            released: 5,
            failed: 1,
            skipped: 3,
            more: true
        }
    );
    let sent = b.bodies.lock().unwrap()[0].clone();
    assert_eq!(sent["prefix"], "home/ltx");
    assert_eq!(sent["olderThanSecs"], 30 * 86_400);
    assert_eq!(sent["maxEntries"], 500);
    let excluded = |v: &Value| {
        v["exclude"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n == "job_812-retention-test-a")
    };
    assert!(excluded(&sent), "{sent}");
    // held by two tasks: still in flight until the last one ends
    drop(working);
    assert!(in_flight_entries().contains(&"job_812-retention-test-a".to_string()));
    drop(twice);
    assert!(!in_flight_entries().contains(&"job_812-retention-test-a".to_string()));
    prune_once(&http, &url, 45).await.unwrap();
    let sent = b.bodies.lock().unwrap()[1].clone();
    assert!(!excluded(&sent));
    assert_eq!(sent["olderThanSecs"], 45 * 86_400);
    assert_eq!(request_body(30, &[])["exclude"], json!([]));
}

#[tokio::test]
async fn a_disabled_period_or_a_failing_bridge_deletes_nothing_and_says_so() {
    // 0 days sent to the bridge -> a request is recorded -> red; a 500 read as success -> red
    let (b, url) = bridge().await;
    let http = reqwest::Client::new();
    assert!(prune_once(&http, &url, 0).await.is_err());
    assert!(b.bodies.lock().unwrap().is_empty(), "nothing was asked");
    *b.answer.lock().unwrap() = Some((500, json!({"error": "Prune failed"})));
    let e = prune_once(&http, &url, 30).await.unwrap_err();
    assert!(e.contains("500"), "{e}");
    *b.answer.lock().unwrap() = Some((200, json!("not counts")));
    assert!(prune_once(&http, &url, 30).await.is_err());
    let e = prune_once(&http, "http://127.0.0.1:1", 30)
        .await
        .unwrap_err();
    assert!(e.contains("unreachable"), "{e}");
}

#[test]
fn every_job_holds_its_result_path_and_the_node_starts_the_daily_run() {
    // the hold removed from the handler, or the start-up call dropped -> red
    let squeeze = |s: &str| -> String { s.chars().filter(|c| !c.is_whitespace()).collect() };
    let handler = squeeze(include_str!("../../src/api/websocket/handlers/ltx.rs"));
    let hold_at = handler
        .find("let_in_flight=crate::ltx::retention::hold(&job_tag);")
        .expect("the handler holds its job");
    let first_write = handler
        .find("format!(\"home/ltx/job_{job_tag}/")
        .expect("the handler writes under the job's folder");
    assert!(hold_at < first_write, "held before the first write");
    let main = squeeze(include_str!("../../src/main.rs"));
    assert!(main.contains("fabstir_llm_node::ltx::retention::spawn(bridge,days);"));
    assert!(main.contains("retention::days_from_lookup(|k|std::env::var(k).ok())"));
    let module = squeeze(include_str!("../../src/ltx/retention.rs"));
    assert!(module.contains("tokio::time::sleep(wait).await"));
    assert!(module.contains("constDAILY:Duration=Duration::from_secs(86_400);"));
}
