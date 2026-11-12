// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! AP2 test support: install the REAL provenance signer (in-process testkit) on a test ApiServer, so the
//! accept path can reserve its manifest slot (D10). Accept-only: the stub never signs.
use fabstir_llm_node::api::server::ApiServer;
use std::sync::Arc;

/// AP2: the real signer in-process, installed on the test server so accept can reserve (D10).
struct InProc {
    ts: provenance_signer::testkit::TestSigner,
    host: String,
}

fn perr(
    f: provenance_signer::service::Fail,
) -> fabstir_llm_node::ltx::provenance::signer_client::SignerError {
    fabstir_llm_node::ltx::provenance::signer_client::SignerError {
        code: Some(f.0),
        message: f.1,
        retryable: f.0.retryable(),
        limits: None,
    }
}

#[async_trait::async_trait]
impl fabstir_llm_node::ltx::provenance::signer_client::ProvenanceSigner for InProc {
    async fn limits(
        &self,
    ) -> Result<
        fabstir_provenance::contract::Limits,
        fabstir_llm_node::ltx::provenance::signer_client::SignerError,
    > {
        Ok(self.ts.svc.limits())
    }
    async fn reserve(
        &self,
        req: &fabstir_provenance::contract::ReserveRequest,
    ) -> Result<
        fabstir_provenance::contract::ReserveResponse,
        fabstir_llm_node::ltx::provenance::signer_client::SignerError,
    > {
        self.ts.svc.reserve(&self.host, req).map_err(perr)
    }
    async fn sign(
        &self,
        _: &fabstir_provenance::contract::SignRequest,
    ) -> Result<Vec<u8>, fabstir_llm_node::ltx::provenance::signer_client::SignerError> {
        unreachable!("accept tests never sign")
    }
    async fn sign_sequence(
        &self,
        _: &fabstir_provenance::contract::SignSequenceRequest,
    ) -> Result<Vec<u8>, fabstir_llm_node::ltx::provenance::signer_client::SignerError> {
        unreachable!("accept tests never sign")
    }
}

pub async fn install_provenance(server: &ApiServer) {
    let host = [0x42u8; 20];
    let ts = provenance_signer::testkit::TestSigner::new_with_min_bundle(
        host,
        &[fabstir_llm_node::version::VERSION],
        0,
    );
    let signer = Arc::new(InProc {
        ts,
        host: hex::encode(host),
    });
    let rt = fabstir_llm_node::ltx::provenance::Runtime {
        signer,
        ffmpeg: fabstir_llm_node::ltx::provenance::ffmpeg::Ffmpeg {
            path: "/unused-at-accept".into(),
        },
        reserve: 12_000,
    };
    server.set_provenance(Arc::new(rt)).await;
}

/// A ComfyUI stand-in that answers `/system_stats` (with or without `--disable-metadata` in argv) and nothing
/// else; accept tests never reach a render.
pub fn comfy_stub(metadata_disabled: bool) -> Arc<fabstir_llm_node::ltx::ComfyClient> {
    use std::io::{Read, Write};
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf);
            let argv = if metadata_disabled {
                r#"["main.py","--highvram","--disable-metadata"]"#
            } else {
                r#"["main.py","--highvram"]"#
            };
            let body = format!(r#"{{"system":{{"argv":{argv}}},"devices":[]}}"#);
            let _ = s.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes());
        }
    });
    Arc::new(fabstir_llm_node::ltx::ComfyClient::new(&format!("http://127.0.0.1:{port}")).unwrap())
}

struct FailingSigner;
#[async_trait::async_trait]
impl fabstir_llm_node::ltx::provenance::signer_client::ProvenanceSigner for FailingSigner {
    async fn limits(
        &self,
    ) -> Result<
        fabstir_provenance::contract::Limits,
        fabstir_llm_node::ltx::provenance::signer_client::SignerError,
    > {
        Err(fabstir_llm_node::ltx::provenance::signer_client::SignerError::transport("down"))
    }
    async fn reserve(
        &self,
        _: &fabstir_provenance::contract::ReserveRequest,
    ) -> Result<
        fabstir_provenance::contract::ReserveResponse,
        fabstir_llm_node::ltx::provenance::signer_client::SignerError,
    > {
        Err(
            fabstir_llm_node::ltx::provenance::signer_client::SignerError::transport(
                "connection refused",
            ),
        )
    }
    async fn sign(
        &self,
        _: &fabstir_provenance::contract::SignRequest,
    ) -> Result<Vec<u8>, fabstir_llm_node::ltx::provenance::signer_client::SignerError> {
        unreachable!()
    }
    async fn sign_sequence(
        &self,
        _: &fabstir_provenance::contract::SignSequenceRequest,
    ) -> Result<Vec<u8>, fabstir_llm_node::ltx::provenance::signer_client::SignerError> {
        unreachable!()
    }
}

/// A runtime whose signer refuses every call (the signer went down after startup).
pub async fn install_failing_provenance(server: &ApiServer) {
    let rt = fabstir_llm_node::ltx::provenance::Runtime {
        signer: Arc::new(FailingSigner),
        ffmpeg: fabstir_llm_node::ltx::provenance::ffmpeg::Ffmpeg {
            path: "/unused-at-accept".into(),
        },
        reserve: 12_000,
    };
    server.set_provenance(Arc::new(rt)).await;
}
