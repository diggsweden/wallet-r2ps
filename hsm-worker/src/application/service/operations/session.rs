// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use super::InnerResponseData;
use super::{OperationContext, OperationResult, ServiceOperation, SessionTransition};
use crate::domain::PakeResponse;
use crate::domain::ServiceRequestError;

pub struct SessionEndOperation;

impl ServiceOperation for SessionEndOperation {
    fn execute(&self, context: OperationContext) -> Result<OperationResult, ServiceRequestError> {
        if context.session_id.is_none() {
            return Err(ServiceRequestError::UnknownSession);
        }

        let payload = PakeResponse { data: None };

        Ok(OperationResult {
            state: None,
            data: InnerResponseData::new(payload)?,
            session_id: context.session_id,
            session_transition: Some(SessionTransition::End),
        })
    }
}
