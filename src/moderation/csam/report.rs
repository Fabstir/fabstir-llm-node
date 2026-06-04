// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! `ReportSink` adapter (NCMEC CyberTipline) — human-confirm, never auto-file.
//!
//! Trait/type *signatures* pinned here (Sub-phase 0.2.4). `MockReportSink` +
//! `NcmecCyberTiplineClient` stub + `prepare_report`/`confirm_and_file` land in
//! Sub-phase 6.2.

use std::sync::Mutex;

use crate::moderation::types::{Category, ModerationError, Result};

/// A report prepared from a quarantined item, awaiting human confirmation before
/// it is filed (B7 — never auto-file). Carries only an opaque case id + category;
/// no raw matched content crosses this boundary.
pub struct PreparedReport {
    pub case_id: String,
    pub category: Category,
}

/// Receipt returned once a report is filed through a [`ReportSink`].
pub struct ReportReceipt {
    pub report_id: String,
}

/// Destination for an abuse report (NCMEC CyberTipline). Mocked in tests; the real
/// `NcmecCyberTiplineClient` swaps in at go-live behind this same trait.
pub trait ReportSink {
    /// File a prepared report. Failure keeps the quarantine item; never clears it.
    fn file(&self, report: PreparedReport) -> Result<ReportReceipt>;
}

/// Proof that a human reviewer approved filing. Filing is impossible without one
/// (type-enforced) — there is NO auto-file path (B7).
pub struct HumanConfirmation {
    pub reviewer: String,
}

impl HumanConfirmation {
    pub fn new(reviewer: impl Into<String>) -> Self {
        Self {
            reviewer: reviewer.into(),
        }
    }
}

/// Prepare a report from a quarantined item (opaque case id + category only — no
/// raw content crosses the boundary).
pub fn prepare_report(case_id: &str, category: Category) -> PreparedReport {
    PreparedReport {
        case_id: case_id.to_string(),
        category,
    }
}

/// File a prepared report — type-enforced to require an explicit [`HumanConfirmation`]:
/// its *construction* is the proof of human confirmation, so the contents are not
/// inspected here and there is NO auto-file path (B7). On sink failure the `Err`
/// propagates so the caller keeps the quarantined item (never clears it). Takes
/// `&dyn` so a server-held `Arc<dyn ReportSink>` can be passed.
pub fn confirm_and_file(
    sink: &dyn ReportSink,
    report: PreparedReport,
    _confirmation: &HumanConfirmation,
) -> Result<ReportReceipt> {
    sink.file(report)
}

/// In-memory `ReportSink` for tests: records filed case ids, returns mock receipts.
#[derive(Default)]
pub struct MockReportSink {
    filed: Mutex<Vec<String>>,
    counter: Mutex<u64>,
}

impl MockReportSink {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn filed_count(&self) -> usize {
        self.filed.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
}

impl ReportSink for MockReportSink {
    fn file(&self, report: PreparedReport) -> Result<ReportReceipt> {
        let mut c = self.counter.lock().unwrap_or_else(|e| e.into_inner());
        *c += 1;
        let report_id = format!("MOCK-REPORT-{c}");
        self.filed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(report.case_id);
        Ok(ReportReceipt { report_id })
    }
}

/// Real NCMEC CyberTipline client — STUB. Until go-live credentials are wired it
/// always fails, so a report is never silently treated as filed (fail-closed).
#[derive(Default)]
pub struct NcmecCyberTiplineClient;

impl NcmecCyberTiplineClient {
    pub fn new() -> Self {
        Self
    }
}

impl ReportSink for NcmecCyberTiplineClient {
    fn file(&self, _report: PreparedReport) -> Result<ReportReceipt> {
        Err(ModerationError::ReportFailed(
            "NCMEC CyberTipline client not configured (go-live)".into(),
        ))
    }
}
