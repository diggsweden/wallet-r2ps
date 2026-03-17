use super::{OperationContext, OperationResult, ServiceOperation, SessionTransition};
use crate::domain::value_objects::r2ps::PakeResponse;
use crate::domain::{InnerResponseData, ServiceRequestError};

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
