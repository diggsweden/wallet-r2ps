use crate::domain::{
    hsm_integration::entities::WorkerResponse,
    request_processing::{errors::RequestError, value_objects::CorrelationId},
};
use crate::ports::outbound::RequestRepository;

/// Result of polling for a request status.
#[derive(Debug)]
pub enum PollResult {
    /// Request is still being processed.
    Pending { correlation_id: CorrelationId },
    /// Request completed successfully.
    Complete {
        correlation_id: CorrelationId,
        response_jws: String,
    },
    /// Request completed with an error.
    Failed {
        correlation_id: CorrelationId,
        http_status: u16,
        message: String,
    },
}

/// Use case: Poll for the status of a previously submitted request.
pub struct PollRequestUseCase<R>
where
    R: RequestRepository,
{
    request_repo: R,
}

impl<R> PollRequestUseCase<R>
where
    R: RequestRepository,
{
    pub fn new(request_repo: R) -> Self {
        Self { request_repo }
    }

    pub async fn execute(&self, correlation_id: CorrelationId) -> Result<PollResult, RequestError> {
        if let Some(response) = self.request_repo.get_response(correlation_id).await? {
            Ok(Self::map_response(correlation_id, &response))
        } else {
            Ok(PollResult::Pending { correlation_id })
        }
    }

    fn map_response(correlation_id: CorrelationId, response: &WorkerResponse) -> PollResult {
        if response.is_success() {
            PollResult::Complete {
                correlation_id,
                response_jws: response.service_response_jws().to_string(),
            }
        } else {
            PollResult::Failed {
                correlation_id,
                http_status: response.http_status(),
                message: "Request failed".to_string(),
            }
        }
    }
}
