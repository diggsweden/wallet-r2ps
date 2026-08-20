// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use std::sync::Arc;

use crate::{
    application::self_test_spi_port::{SelfTestError, SelfTestProbe, TsfClaim},
    infrastructure::hsm_wrapper::HsmWrapper,
};

pub struct HsmRoundtripProbe {
    hsm: Arc<HsmWrapper>,
    root_key_label: Option<String>,
    domain_separator: Option<String>,
}

impl HsmRoundtripProbe {
    pub fn new(
        hsm: Arc<HsmWrapper>,
        root_key_label: Option<String>,
        domain_separator: Option<String>,
    ) -> Self {
        Self {
            hsm,
            root_key_label,
            domain_separator,
        }
    }
}

impl SelfTestProbe for HsmRoundtripProbe {
    fn name(&self) -> &'static str {
        "hsm_roundtrip"
    }

    fn claim(&self) -> TsfClaim {
        TsfClaim::WscdHsmConnectivity
    }

    fn probe(&self) -> Result<(), SelfTestError> {
        self.hsm.check_wrap_sign().map_err(|e| SelfTestError {
            detail: format!("hsm_roundtrip: wrap-sign failed: {e}"),
        })?;

        if let (Some(label), Some(sep)) = (&self.root_key_label, &self.domain_separator) {
            self.hsm
                .check_derivation(label, sep)
                .map_err(|e| SelfTestError {
                    detail: format!("hsm_roundtrip: derivation failed: {e}"),
                })?;
        }
        Ok(())
    }
}
