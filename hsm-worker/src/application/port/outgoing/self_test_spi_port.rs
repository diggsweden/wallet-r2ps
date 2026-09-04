// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

/// Which FPT_TST.1 Application Note 30 requirement a check is evidence for.
///
/// `TsfClaim` names the requirement category being demonstrated, not the check that
/// demonstrates it — several `SelfTestProbe`s can evidence the same claim (e.g. both KAT probes
/// evidence `CryptographicLibraries`). The specific check that ran is
/// `SelfTestProbe::name` / `CheckResult::name`, which is finer-grained than `claim`.
///
/// TLS is absent because this service does not terminate the RAC — see §2.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TsfClaim {
    CryptographicLibraries,
    WscdHsmConnectivity,
    CredentialStoreIntegrity,
    AuditLogAvailability,
    TransactionIdentifierRegistry,
}

impl TsfClaim {
    pub const ALL: [TsfClaim; 5] = [
        TsfClaim::CryptographicLibraries,
        TsfClaim::WscdHsmConnectivity,
        TsfClaim::CredentialStoreIntegrity,
        TsfClaim::AuditLogAvailability,
        TsfClaim::TransactionIdentifierRegistry,
    ];
}

/// What caused this suite run.
///
/// On-demand is not yet represented. FAU_GEN.2.1 requires user-initiated events to carry the
/// requesting identity — so that variant will carry it, and the audit record has to distinguish
/// all three cases.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    Startup,
    Periodic,
}

/// Why a check failed.
///
/// `detail` reaches an audit record. It must never carry key material, PINs or
/// OPAQUE state — record the operation that failed, not the data it saw.
#[derive(Debug, PartialEq, Eq)]
pub struct SelfTestError {
    pub detail: String,
}

/// FAU_GEN.1.2(a) requires the outcome of each auditable event: success or failure.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Pass,
    Fail(SelfTestError),
    /// No `SelfTestProbe` evidences this claim yet. Not a pass and not a
    /// runtime failure — `TsfHealth` must not gate on it — but it has to
    /// reach the audit trail so "N checks, all passed" can't be mistaken
    /// for full AN 30 coverage.
    NotImplemented,
}

/// Audited under FAU_GEN.1.1(i).
#[derive(Debug, PartialEq, Eq)]
pub struct CheckResult {
    /// Event type for the audit record; finer-grained than `claim`.
    pub name: &'static str,
    /// The Application Note 30 item this check is evidence for.
    pub claim: TsfClaim,
    pub outcome: Outcome,
}

pub trait SelfTestProbe: Send + Sync {
    fn name(&self) -> &'static str;
    fn claim(&self) -> TsfClaim;
    fn probe(&self) -> Result<(), SelfTestError>;
}
