// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 5 P5.5 (design §7, `test_http_sources.rs` rows) — the streaming
//! `HttpBlobSource::get_file_to` against three loopback fakes: hyper
//! streaming a file (with `Content-Length`), hyper chunked (without), and a
//! raw TCP server that can lie about `Content-Length`, stall, or never
//! answer. Plus the pure same-filesystem space rule.

use fabstir_llm_node::tee::container_cache::{check_space_for_download, FetchHooks, FsSpace};
use fabstir_llm_node::tee::http_sources::HttpBlobSource;
use fabstir_llm_node::tee::model_source::BlobSource;
use fabstir_llm_node::tee::types::TeeError;
use futures::TryStreamExt;
use http_body_util::{combinators::BoxBody, BodyExt, StreamBody};
use hyper::body::{Bytes, Frame, Incoming};
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::AsyncWriteExt;

const WRAP: Duration = Duration::from_secs(5);
const MIB: usize = 1024 * 1024;

/// hyper 1.x serving `file` as a streamed body; `with_length` sets
/// `Content-Length` (nginx static), else hyper sends it chunked.
async fn spawn_streaming(file: PathBuf, with_length: bool) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((tcp, _)) = listener.accept().await else {
                break;
            };
            let file = file.clone();
            tokio::spawn(async move {
                let svc = service_fn(move |_req: Request<Incoming>| {
                    let file = file.clone();
                    async move {
                        let len = std::fs::metadata(&file).unwrap().len();
                        let f = tokio::fs::File::open(&file).await.unwrap();
                        let stream = tokio_util::io::ReaderStream::new(f).map_ok(Frame::data);
                        let body: BoxBody<Bytes, std::io::Error> =
                            BodyExt::boxed(StreamBody::new(stream));
                        let mut resp = Response::builder().status(200);
                        if with_length {
                            resp = resp.header("content-length", len.to_string());
                        }
                        Ok::<_, std::convert::Infallible>(resp.body(body).unwrap())
                    }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(tcp), svc)
                    .await;
            });
        }
    });
    addr
}

/// A raw TCP server: reads the request head, writes `head` then `body`, then
/// either closes (`stall == false`) or holds the connection open forever.
async fn spawn_raw(head: String, body: Vec<u8>, stall: bool) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut tcp, _)) = listener.accept().await else {
                break;
            };
            let head = head.clone();
            let body = body.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let _ = tokio::io::AsyncReadExt::read(&mut tcp, &mut buf).await;
                let _ = tcp.write_all(head.as_bytes()).await;
                let _ = tcp.write_all(&body).await;
                let _ = tcp.flush().await;
                if stall {
                    std::future::pending::<()>().await;
                }
            });
        }
    });
    addr
}

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

#[tokio::test]
async fn get_file_to_streams_byte_exact_and_replaces_a_stale_part() {
    tokio::time::timeout(WRAP, async {
        let tmp = tempfile::tempdir().unwrap();
        let body: Vec<u8> = (0..3 * MIB).map(|i| (i % 251) as u8).collect();
        let src_file = tmp.path().join("blob.enc");
        std::fs::write(&src_file, &body).unwrap();
        let addr = spawn_streaming(src_file, true).await;
        let s = source(addr, 8 * MIB as u64, 2000);
        let dest = tmp.path().join("cache").join("k.enc");
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        let n = s.get_file_to("blob.enc", &dest, hooks()).await.unwrap();
        assert_eq!(n, 3 * MIB as u64);
        assert_eq!(std::fs::read(&dest).unwrap(), body, "byte-exact");
        assert!(
            parts(dest.parent().unwrap()).is_empty(),
            "no .part of its own left"
        );
        // A stale foreign part with other bytes: dest is still the body.
        std::fs::write(dest.parent().unwrap().join("k.enc.deadbeef.part"), b"stale").unwrap();
        s.get_file_to("blob.enc", &dest, hooks()).await.unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), body);
    })
    .await
    .expect("hung");
}

#[tokio::test]
async fn get_file_to_refuses_before_any_byte_on_length_space_and_headers() {
    tokio::time::timeout(WRAP, async {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("k.enc");
        // Content-Length above the bound.
        let addr = spawn_raw(
            "HTTP/1.1 200 OK\r\nContent-Length: 5000000\r\n\r\n".into(),
            vec![0u8; 64],
            true,
        )
        .await;
        let err = source(addr, 1000, 500)
            .get_file_to("x", &dest, hooks())
            .await
            .expect_err("over the bound");
        assert!(err.to_string().contains("5000000 bytes exceeds"), "{err}");
        assert!(!dest.exists() && parts(tmp.path()).is_empty());
        // No Content-Length (chunked).
        let src_file = tmp.path().join("blob.enc");
        std::fs::write(&src_file, vec![1u8; 4096]).unwrap();
        let addr = spawn_streaming(src_file, false).await;
        let err = source(addr, 1 << 20, 500)
            .get_file_to("blob.enc", &dest, hooks())
            .await
            .expect_err("chunked is refused");
        assert!(err.to_string().contains("no Content-Length"), "{err}");
        assert!(!dest.exists() && parts(tmp.path()).is_empty());
        // The loader's space check says no.
        let addr = spawn_raw(
            "HTTP/1.1 200 OK\r\nContent-Length: 64\r\n\r\n".into(),
            vec![0u8; 64],
            false,
        )
        .await;
        let refuse = |len: u64| Err(TeeError::Fetch(format!("needs {len}")));
        let err = source(addr, 1 << 20, 500)
            .get_file_to(
                "x",
                &dest,
                FetchHooks {
                    on_length: &refuse,
                    on_head: &fabstir_llm_node::tee::container_cache::accept_head,
                },
            )
            .await
            .expect_err("space");
        assert!(err.to_string().contains("needs 64"), "{err}");
        assert!(!dest.exists() && parts(tmp.path()).is_empty());
        // The head hook refuses after the first bytes: no .part, the refusal
        // is the hook's own error.
        let addr = spawn_raw(
            format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", 4 * MIB),
            vec![9u8; 4 * MIB],
            false,
        )
        .await;
        let refuse_head = |h: &[u8]| {
            Err(TeeError::VerificationFailed(format!(
                "head refused ({} bytes)",
                h.len()
            )))
        };
        let err = source(addr, 8 * MIB as u64, 500)
            .get_file_to(
                "x",
                &dest,
                FetchHooks {
                    on_length: &fabstir_llm_node::tee::container_cache::accept_length,
                    on_head: &refuse_head,
                },
            )
            .await
            .expect_err("head");
        assert!(err.to_string().contains("head refused (98 bytes)"), "{err}");
        assert!(!dest.exists() && parts(tmp.path()).is_empty());
        // A server that accepts and never answers: the idle timeout covers
        // the headers too.
        let addr = spawn_raw(String::new(), Vec::new(), true).await;
        let started = std::time::Instant::now();
        let err = source(addr, 1 << 20, 300)
            .get_file_to("x", &dest, hooks())
            .await
            .expect_err("no headers");
        assert!(err.to_string().contains("no response headers"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(1));
    })
    .await
    .expect("hung");
}

#[tokio::test]
async fn get_file_to_refuses_a_short_body_an_idle_stream_and_a_dropped_future() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("k.enc");
        // Declares 4 MiB, sends 3 MiB, closes.
        let addr = spawn_raw(
            format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", 4 * MIB),
            vec![7u8; 3 * MIB],
            false,
        )
        .await;
        let err = source(addr, 8 * MIB as u64, 2000)
            .get_file_to("x", &dest, hooks())
            .await
            .expect_err("short body");
        assert!(matches!(err, TeeError::Fetch(_)), "{err:?}");
        assert!(!dest.exists(), "no dest");
        assert!(parts(tmp.path()).is_empty(), "the .part is unlinked");
        // Sends 1 MiB then stalls, on every connection (it ignores Range and
        // sends no validator, so each resume is a restart from zero and
        // stalls again): idle 200 ms × (1 + 3 restarts), then it is final.
        let addr = spawn_raw(
            format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", 4 * MIB),
            vec![7u8; MIB],
            true,
        )
        .await;
        let started = std::time::Instant::now();
        let err = source(addr, 8 * MIB as u64, 200)
            .get_file_to("x", &dest, hooks())
            .await
            .expect_err("idle");
        let m = err.to_string();
        assert!(m.contains("restarted from byte zero 3 times"), "{m}");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
        assert!(!dest.exists() && parts(tmp.path()).is_empty());
        // A dropped future (idle 10 s, so its own Elapsed never runs first):
        // wait for the .part to EXIST, then drop → the guard unlinks it.
        let addr = spawn_raw(
            format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", 4 * MIB),
            vec![7u8; MIB],
            true,
        )
        .await;
        let s = source(addr, 8 * MIB as u64, 10_000);
        let mut load = Box::pin(s.get_file_to("x", &dest, hooks()));
        let dir = tmp.path().to_path_buf();
        let seen = async {
            loop {
                if !parts(&dir).is_empty() {
                    return;
                }
                tokio::task::yield_now().await;
            }
        };
        tokio::select! {
            r = &mut load => panic!("the download finished: {r:?}"),
            _ = seen => {}
        }
        assert!(
            !parts(tmp.path()).is_empty(),
            "the .part exists while the future lives"
        );
        drop(load);
        assert!(
            parts(tmp.path()).is_empty(),
            "dropping the future unlinks the .part"
        );
        assert!(!dest.exists());
    })
    .await
    .expect("hung");
}

#[test]
fn check_space_requires_twice_the_length_on_one_filesystem() {
    let a = Path::new("/a");
    let b = Path::new("/b");
    let len = 1000u64;
    let different = |p: &Path| -> Result<FsSpace, TeeError> {
        Ok(FsSpace {
            dev: if p == Path::new("/a") { 1 } else { 2 },
            avail: len,
        })
    };
    check_space_for_download(&different, a, b, len).expect("1× each on two filesystems");
    let same_short = |_: &Path| -> Result<FsSpace, TeeError> {
        Ok(FsSpace {
            dev: 1,
            avail: len + len / 2,
        })
    };
    let err = check_space_for_download(&same_short, a, b, len).expect_err("1.5× on one fs");
    let m = err.to_string();
    assert!(m.contains("needs") && m.contains("same filesystem"), "{m}");
    let same_ok = |_: &Path| -> Result<FsSpace, TeeError> {
        Ok(FsSpace {
            dev: 1,
            avail: 2 * len,
        })
    };
    check_space_for_download(&same_ok, a, b, len).expect("2× on one fs");
}
