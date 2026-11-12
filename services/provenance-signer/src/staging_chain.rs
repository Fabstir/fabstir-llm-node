// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! `provenance-signer gen-staging-chain`: a STAGING root (and optional issuing CA) plus an end-entity
//! claim-signing certificate meeting c2pa-rs's EE profile checks (AP0.2 `gen-staging-chain.sh`):
//! P-256, ecdsa-with-SHA256, EE not CA, KeyUsage digitalSignature (critical), EKU documentSigning
//! (1.3.6.1.5.5.7.3.36), SKI + AKI. The EE key is written ONLY as encrypted PKCS#8; CA keys are discarded.

use crate::keystore::encrypt_pkcs8_pem;
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa,
    KeyIdMethod, KeyPair, KeyUsagePurpose, PKCS_ECDSA_P256_SHA256,
};
use std::path::Path;
use time::{Duration, OffsetDateTime};

const ORG: &str = "Fabstir Services Limited (STAGING)";
/// id-kp-documentSigning (RFC 9336).
const EKU_DOCUMENT_SIGNING: &[u64] = &[1, 3, 6, 1, 5, 5, 7, 3, 36];

pub struct StagingChain {
    pub root_pem: String,
    pub intermediate_pem: Option<String>,
    pub ee_pem: String,
    /// EE first, root excluded (C2PA 14.5).
    pub chain_pem: String,
    pub ee_key_encrypted_pem: zeroize::Zeroizing<String>,
}

fn dn(cn: &str) -> DistinguishedName {
    let mut d = DistinguishedName::new();
    d.push(DnType::CountryName, "GB");
    d.push(DnType::OrganizationName, ORG);
    d.push(DnType::CommonName, cn);
    d
}

fn ca_params(cn: &str, pathlen: u8) -> CertificateParams {
    let mut p = CertificateParams::default();
    p.distinguished_name = dn(cn);
    p.is_ca = IsCa::Ca(BasicConstraints::Constrained(pathlen));
    p.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    p.not_before = OffsetDateTime::now_utc() - Duration::days(1);
    p.not_after = OffsetDateTime::now_utc() + Duration::days(3650);
    p.key_identifier_method = KeyIdMethod::Sha256;
    p
}

/// Generate the chain. `ee_days` = EE validity (397 like a public CA). `eku` overrides the EE EKU for the
/// negative profile tests.
pub fn generate(
    with_intermediate: bool,
    passphrase: &[u8],
    ee_days: i64,
    eku: Option<Vec<ExtendedKeyUsagePurpose>>,
) -> anyhow::Result<StagingChain> {
    anyhow::ensure!(
        passphrase.len() >= 12,
        "passphrase must be at least 12 bytes"
    );
    let root_key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256)?;
    let root = ca_params("Platformless AI Staging Root CA", 1).self_signed(&root_key)?;

    let int = if with_intermediate {
        let int_key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256)?;
        let mut ip = ca_params("Platformless AI Staging Issuing CA", 0);
        ip.use_authority_key_identifier_extension = true;
        let int = ip.signed_by(&int_key, &root, &root_key)?;
        Some((int, int_key))
    } else {
        None
    };
    let (issuer_cert, issuer_key) = match &int {
        Some((c, k)) => (c, k),
        None => (&root, &root_key),
    };
    let int_pem = int.as_ref().map(|(c, _)| c.pem());

    let ee_sk = p256::ecdsa::SigningKey::random(&mut rand::rngs::OsRng);
    let ee_der = p256::pkcs8::EncodePrivateKey::to_pkcs8_der(&ee_sk)?;
    let ee_key = KeyPair::try_from(ee_der.as_bytes())?;
    let mut ep = CertificateParams::default();
    ep.distinguished_name = dn("Platformless AI Staging Signer");
    ep.is_ca = IsCa::ExplicitNoCa;
    ep.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    ep.extended_key_usages = eku.unwrap_or_else(|| {
        vec![ExtendedKeyUsagePurpose::Other(
            EKU_DOCUMENT_SIGNING.to_vec(),
        )]
    });
    ep.use_authority_key_identifier_extension = true;
    ep.key_identifier_method = KeyIdMethod::Sha256;
    ep.not_before = OffsetDateTime::now_utc() - Duration::days(1);
    ep.not_after = OffsetDateTime::now_utc() + Duration::days(ee_days);
    let ee = ep.signed_by(&ee_key, issuer_cert, issuer_key)?;

    let ee_pem = ee.pem();
    let mut chain_pem = ee_pem.clone();
    if let Some(i) = &int_pem {
        chain_pem.push_str(i);
    }
    Ok(StagingChain {
        root_pem: root.pem(),
        intermediate_pem: int_pem,
        ee_pem,
        chain_pem,
        ee_key_encrypted_pem: encrypt_pkcs8_pem(&ee_sk, passphrase)?,
    })
}

/// Write the chain files; refuses to overwrite. Never prints the key.
pub fn write(out: &Path, c: &StagingChain) -> anyhow::Result<()> {
    std::fs::create_dir_all(out)?;
    let files: Vec<(&str, &str)> = vec![
        ("root.pem", &c.root_pem),
        ("anchors.pem", &c.root_pem),
        ("ee.pem", &c.ee_pem),
        ("chain.pem", &c.chain_pem),
        ("ee.key.enc.pem", c.ee_key_encrypted_pem.as_str()),
    ];
    for (name, body) in files
        .into_iter()
        .chain(c.intermediate_pem.as_deref().map(|i| ("int.pem", i)))
    {
        use std::io::Write;
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts
            .open(out.join(name))
            .map_err(|e| anyhow::anyhow!("{name}: {e}"))?;
        f.write_all(body.as_bytes())?;
    }
    Ok(())
}
