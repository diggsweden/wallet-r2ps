// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use std::sync::Arc;

use rstest::rstest;

use crate::application::port::outgoing::self_test_spi_port::SelfTestProbe;

use crate::application::self_test_spi_port::TsfClaim;
use crate::application::session_state_spi_port::SessionTransition;
use crate::infrastructure::self_test_probes::test_utils::FakeSessionStore;
use crate::{
    application::session_state_spi_port::SessionStateSpiPort,
    infrastructure::{
        self_test_probes::credential_store_roundtrip::CredentialStoreRoundtripProbe,
        session_state_memory_cache::SessionStateMemoryCache,
    },
};

#[test]
fn probe_passes_twice_in_a_row_leaving_no_entries_behind() {
    let cache: Arc<dyn SessionStateSpiPort> = Arc::new(SessionStateMemoryCache::new());
    let probe = CredentialStoreRoundtripProbe::new(cache);

    assert!(probe.probe().is_ok());
    assert!(probe.probe().is_ok());
}

#[test]
fn probe_reports_the_correct_name_and_claim() {
    let probe = CredentialStoreRoundtripProbe::new(Arc::new(SessionStateMemoryCache::new()));
    assert_eq!(probe.name(), "credential_store_roundtrip");
    assert_eq!(probe.claim(), TsfClaim::CredentialStoreIntegrity);
}

#[test]
fn probe_fails_when_store_acks_writes_without_persisting() {
    let probe =
        CredentialStoreRoundtripProbe::new(Arc::new(FakeSessionStore::acks_without_persisting()));
    let err = probe.probe().unwrap_err();

    assert_eq!(
        err.detail,
        "credential_store_roundtrip: expected PendingAuth after write, got something else"
    );
}

#[rstest]
#[case::create_pending_auth(
    |t: &SessionTransition| matches!(t, SessionTransition::CreatePendingAuth {..}),
    "create pending auth"
)]
#[case::authenticate(
    |t: &SessionTransition| matches!(t, SessionTransition::Authenticate {..}),
    "authenticate transition"
)]
#[case::end(
    |t: &SessionTransition| matches!(t, SessionTransition::End),
    "end transition rejected"
)]
fn probe_fails_when_a_transition_is_rejected(
    #[case] reject: fn(&SessionTransition) -> bool,
    #[case] expected_detail: &str,
) {
    let probe = CredentialStoreRoundtripProbe::new(Arc::new(FakeSessionStore::rejecting(reject)));
    let err = probe.probe().unwrap_err();

    assert!(
        err.detail.contains(expected_detail),
        "expected detail mentioning {expected_detail:?}, got {:?}",
        err.detail
    );
}
