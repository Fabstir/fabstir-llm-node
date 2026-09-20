// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 5 P5.5 (code-review round 3) — the streaming download's bounded
//! `Range` resume: a body error mid-stream (a reset, an nginx reload) resumes
//! from the byte count in the same `.part` instead of restarting tens of GB
//! from zero and burning one of docker's five restarts; a host that ignores
//! `Range` restarts in place; five resumes and the error is final.

use fabstir_llm_node::tee::container_cache::FetchHooks;
use fabstir_llm_node::tee::http_sources::HttpBlobSource;
use fabstir_llm_node::tee::model_source::BlobSource;
use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

const MIB: usize = 1024 * 1024;

fn source(addr: SocketAddr, max: u64, idle_ms: u64) -> HttpBlobSource {
    HttpBlobSource::new(&format!("http://{addr}"), max, Duration::from_secs(60))
        .unwrap()
        .with_idle_timeout(Duration::from_millis(idle_ms))
}

fn parts(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".part"))
        .collect()
}

fn hooks() -> FetchHooks<'static> {
    FetchHooks::accept_all()
}

/// A raw server driven per connection: `script(conn_no, range_start)` gives
/// the head and body to write and whether to STALL afterwards (hold the
/// connection open forever) instead of closing it.
type Script = std::sync::Arc<dyn Fn(usize, Option<u64>) -> (String, Vec<u8>, bool) + Send + Sync>;

/// Every `If-Range` value the scripted server has seen (one test binary,
/// tests run single-threaded here; cleared by the test that asserts on it).
static IF_RANGE_SEEN: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

async fn spawn_scripted(
    script: Script,
) -> (SocketAddr, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let conns = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = conns.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut tcp, _)) = listener.accept().await else {
                break;
            };
            let n = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let script = script.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let got = tokio::io::AsyncReadExt::read(&mut tcp, &mut buf)
                    .await
                    .unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..got]).to_string();
                let range = head
                    .lines()
                    .find_map(|l| {
                        l.strip_prefix("range: ")
                            .or_else(|| l.strip_prefix("Range: "))
                    })
                    .and_then(|v| v.trim().strip_prefix("bytes="))
                    .and_then(|v| v.trim_end_matches('-').parse::<u64>().ok());
                if let Some(v) = head.lines().find_map(|l| {
                    l.strip_prefix("if-range: ")
                        .or_else(|| l.strip_prefix("If-Range: "))
                }) {
                    IF_RANGE_SEEN.lock().unwrap().push(v.trim().to_string());
                }
                let (h, b, stall) = script(n, range);
                let _ = tcp.write_all(h.as_bytes()).await;
                let _ = tcp.write_all(&b).await;
                let _ = tcp.flush().await;
                if stall {
                    std::future::pending::<()>().await;
                }
            });
        }
    });
    (addr, conns)
}

#[tokio::test]
async fn a_body_error_mid_stream_resumes_with_a_range_request() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("k.enc");
        let body: Vec<u8> = (0..4 * MIB).map(|i| (i % 253) as u8).collect();
        let len = body.len();
        // Connection 0: declares the whole body, sends 1 MiB, closes.
        // Connection 1: honours the Range with a 206 from exactly that offset.
        let full = body.clone();
        let script: Script = std::sync::Arc::new(move |n, range| match (n, range) {
            (0, _) => (
                format!("HTTP/1.1 200 OK\r\nETag: \"x\"\r\nContent-Length: {len}\r\n\r\n"),
                full[..MIB].to_vec(),
                false,
            ),
            (_, Some(start)) => (
                format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{}/{len}\r\nContent-Length: {}\r\n\r\n",
                    len - 1,
                    len as u64 - start
                ),
                full[start as usize..].to_vec(),
                false,
            ),
            (_, None) => (
                "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n".into(),
                vec![],
                false,
            ),
        });
        let (addr, conns) = spawn_scripted(script).await;
        let n = source(addr, 8 * MIB as u64, 2000)
            .get_file_to("x", &dest, hooks())
            .await
            .expect("resumed");
        assert_eq!(n, len as u64);
        assert_eq!(std::fs::read(&dest).unwrap(), body, "byte-exact across the resume");
        assert_eq!(conns.load(std::sync::atomic::Ordering::SeqCst), 2, "exactly one resume");
        assert!(parts(tmp.path()).is_empty());

        // A host that always cuts after 1 MiB and ignores Range (200 every
        // time): five resumes, then the error is final and the .part is gone.
        let dest2 = tmp.path().join("k2.enc");
        let full = body.clone();
        let script: Script = std::sync::Arc::new(move |_, _| {
            (
                format!("HTTP/1.1 200 OK\r\nETag: \"x\"\r\nContent-Length: {len}\r\n\r\n"),
                full[..MIB].to_vec(),
                false,
            )
        });
        let (addr, conns) = spawn_scripted(script).await;
        let err = source(addr, 8 * MIB as u64, 2000)
            .get_file_to("x", &dest2, hooks())
            .await
            .expect_err("bounded");
        assert!(
            err.to_string().contains("restarted from byte zero 3 times"),
            "{err}"
        );
        assert_eq!(
            conns.load(std::sync::atomic::Ordering::SeqCst),
            5,
            "1 + 3 restarts + the 4th restart's GET that trips the bound"
        );
        assert!(!dest2.exists() && parts(tmp.path()).is_empty());

        // A 206 from the wrong offset is refused, never appended.
        let dest3 = tmp.path().join("k3.enc");
        let full = body.clone();
        let script: Script = std::sync::Arc::new(move |n, _| match n {
            0 => (
                format!("HTTP/1.1 200 OK\r\nETag: \"x\"\r\nContent-Length: {len}\r\n\r\n"),
                full[..MIB].to_vec(),
                false,
            ),
            _ => (
                format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-{}/{len}\r\nContent-Length: {len}\r\n\r\n",
                    len - 1
                ),
                full.clone(),
                false,
            ),
        });
        let (addr, _) = spawn_scripted(script).await;
        let err = source(addr, 8 * MIB as u64, 2000)
            .get_file_to("x", &dest3, hooks())
            .await
            .expect_err("wrong offset");
        assert!(err.to_string().contains("Content-Range"), "{err}");
        assert!(!dest3.exists() && parts(tmp.path()).is_empty());

        // A 206 honouring only PART of the remainder (end < len-1) is refused
        // up front, never discovered as a short body after the fetch.
        let dest4 = tmp.path().join("k4.enc");
        let full = body.clone();
        let script: Script = std::sync::Arc::new(move |n, range| match (n, range) {
            (0, _) => (
                format!("HTTP/1.1 200 OK\r\nETag: \"x\"\r\nContent-Length: {len}\r\n\r\n"),
                full[..MIB].to_vec(),
                false,
            ),
            (_, Some(start)) => {
                let end = start as usize + MIB - 1;
                (
                    format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{end}/{len}\r\nContent-Length: {MIB}\r\n\r\n"
                    ),
                    full[start as usize..=end].to_vec(),
                    false,
                )
            }
            _ => ("HTTP/1.1 500 X\r\nContent-Length: 0\r\n\r\n".into(), vec![], false),
        });
        let (addr, _) = spawn_scripted(script).await;
        let err = source(addr, 8 * MIB as u64, 2000)
            .get_file_to("x", &dest4, hooks())
            .await
            .expect_err("partial 206");
        assert!(err.to_string().contains("Content-Range"), "{err}");
        assert!(!dest4.exists() && parts(tmp.path()).is_empty());

        // A STALL mid-body resumes too: connection 0 sends 1 MiB and hangs;
        // after the idle timeout the resume's 206 completes the download.
        let dest5 = tmp.path().join("k5.enc");
        let full = body.clone();
        let script: Script = std::sync::Arc::new(move |n, range| match (n, range) {
            (0, _) => (
                format!("HTTP/1.1 200 OK\r\nETag: \"x\"\r\nContent-Length: {len}\r\n\r\n"),
                full[..MIB].to_vec(),
                true,
            ),
            (_, Some(start)) => (
                format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{}/{len}\r\nContent-Length: {}\r\n\r\n",
                    len - 1,
                    len as u64 - start
                ),
                full[start as usize..].to_vec(),
                false,
            ),
            _ => ("HTTP/1.1 500 X\r\nContent-Length: 0\r\n\r\n".into(), vec![], false),
        });
        let (addr, conns) = spawn_scripted(script).await;
        let n = source(addr, 8 * MIB as u64, 300)
            .get_file_to("x", &dest5, hooks())
            .await
            .expect("a stall resumes");
        assert_eq!(n, len as u64);
        assert_eq!(std::fs::read(&dest5).unwrap(), body);
        assert_eq!(conns.load(std::sync::atomic::Ordering::SeqCst), 2);

        // If-Range: the first 200 carries an ETag; the resume must send it
        // back, and a host whose object CHANGED answers 200 (same size), which
        // restarts in place: the file is the NEW object, never a splice.
        IF_RANGE_SEEN.lock().unwrap().clear();
        let dest6 = tmp.path().join("k6.enc");
        let old = body.clone();
        let new_body: Vec<u8> = (0..4 * MIB).map(|i| (i % 241) as u8).collect();
        let new_full = new_body.clone();
        let script: Script = std::sync::Arc::new(move |n, _| match n {
            0 => (
                format!(
                    "HTTP/1.1 200 OK\r\nETag: \"seal-v1\"\r\nContent-Length: {len}\r\n\r\n"
                ),
                old[..MIB].to_vec(),
                false,
            ),
            _ => (
                format!(
                    "HTTP/1.1 200 OK\r\nETag: \"seal-v2\"\r\nContent-Length: {len}\r\n\r\n"
                ),
                new_full.clone(),
                false,
            ),
        });
        let (addr, conns) = spawn_scripted(script).await;
        let n = source(addr, 8 * MIB as u64, 2000)
            .get_file_to("x", &dest6, hooks())
            .await
            .expect("restarted on the changed object");
        assert_eq!(n, len as u64);
        assert_eq!(
            std::fs::read(&dest6).unwrap(),
            new_body,
            "the new object, not a splice"
        );
        assert_eq!(conns.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert_eq!(
            IF_RANGE_SEEN.lock().unwrap().clone(),
            vec!["\"seal-v1\"".to_string()],
            "the resume carried the first response's ETag as If-Range"
        );

        // After a restart the validator is the NEW object's: connection 1
        // answers 200 with ETag v2 and cuts at 2 MiB; connection 2 must see
        // If-Range v2 and completes with a 206 of the new body.
        IF_RANGE_SEEN.lock().unwrap().clear();
        let dest7 = tmp.path().join("k7.enc");
        let old = body.clone();
        let new_full = new_body.clone();
        let script: Script = std::sync::Arc::new(move |n, range| match (n, range) {
            (0, _) => (
                format!(
                    "HTTP/1.1 200 OK\r\nETag: \"seal-v1\"\r\nContent-Length: {len}\r\n\r\n"
                ),
                old[..MIB].to_vec(),
                false,
            ),
            (1, _) => (
                format!(
                    "HTTP/1.1 200 OK\r\nETag: \"seal-v2\"\r\nContent-Length: {len}\r\n\r\n"
                ),
                new_full[..2 * MIB].to_vec(),
                false,
            ),
            (_, Some(start)) => (
                format!(
                    "HTTP/1.1 206 Partial Content\r\nETag: \"seal-v2\"\r\nContent-Range: bytes {start}-{}/{len}\r\nContent-Length: {}\r\n\r\n",
                    len - 1,
                    len as u64 - start
                ),
                new_full[start as usize..].to_vec(),
                false,
            ),
            _ => ("HTTP/1.1 500 X\r\nContent-Length: 0\r\n\r\n".into(), vec![], false),
        });
        let (addr, conns) = spawn_scripted(script).await;
        source(addr, 8 * MIB as u64, 2000)
            .get_file_to("x", &dest7, hooks())
            .await
            .expect("restart then resume under the new validator");
        assert_eq!(std::fs::read(&dest7).unwrap(), new_body);
        assert_eq!(conns.load(std::sync::atomic::Ordering::SeqCst), 3);
        assert_eq!(
            IF_RANGE_SEEN.lock().unwrap().clone(),
            vec!["\"seal-v1\"".to_string(), "\"seal-v2\"".to_string()],
            "the second resume carried the NEW object's ETag"
        );

        // A resume GET that itself fails (the host closes without a head
        // during the same blip) spends one resume and is retried: connection
        // 1 answers nothing, connection 2 honours the range.
        let dest8 = tmp.path().join("k8.enc");
        let full = body.clone();
        let script: Script = std::sync::Arc::new(move |n, range| match (n, range) {
            (0, _) => (
                format!("HTTP/1.1 200 OK\r\nETag: \"x\"\r\nContent-Length: {len}\r\n\r\n"),
                full[..MIB].to_vec(),
                false,
            ),
            (1, _) => (String::new(), vec![], false),
            (_, Some(start)) => (
                format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{}/{len}\r\nContent-Length: {}\r\n\r\n",
                    len - 1,
                    len as u64 - start
                ),
                full[start as usize..].to_vec(),
                false,
            ),
            _ => ("HTTP/1.1 500 X\r\nContent-Length: 0\r\n\r\n".into(), vec![], false),
        });
        let (addr, conns) = spawn_scripted(script).await;
        source(addr, 8 * MIB as u64, 300)
            .get_file_to("x", &dest8, hooks())
            .await
            .expect("a failed resume GET is retried");
        assert_eq!(std::fs::read(&dest8).unwrap(), body);
        assert_eq!(conns.load(std::sync::atomic::Ordering::SeqCst), 3);

        // No validator at all (a front stripping ETag and Last-Modified): the
        // interruption is NOT resumed with Range (a same-size replaced object
        // would splice); a plain GET restarts in place within the attempt.
        let dest9 = tmp.path().join("k9.enc");
        let full = body.clone();
        let script: Script = std::sync::Arc::new(move |n, range| match (n, range) {
            (0, _) => (
                format!("HTTP/1.1 200 OK\r\nContent-Length: {len}\r\n\r\n"),
                full[..MIB].to_vec(),
                false,
            ),
            (_, None) => (
                format!("HTTP/1.1 200 OK\r\nContent-Length: {len}\r\n\r\n"),
                full.clone(),
                false,
            ),
            (_, Some(_)) => (
                "HTTP/1.1 500 X\r\nContent-Length: 0\r\n\r\n".into(),
                vec![],
                false,
            ),
        });
        let (addr, conns) = spawn_scripted(script).await;
        source(addr, 8 * MIB as u64, 2000)
            .get_file_to("x", &dest9, hooks())
            .await
            .expect("restarted without Range");
        assert_eq!(std::fs::read(&dest9).unwrap(), body);
        assert_eq!(conns.load(std::sync::atomic::Ordering::SeqCst), 2);
    })
    .await
    .expect("hung");
}
