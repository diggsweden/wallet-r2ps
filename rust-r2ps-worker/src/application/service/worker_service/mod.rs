pub mod context;
pub mod decode;
pub mod error;
pub mod response;

#[cfg(test)]
mod tests;

pub use context::{ResponseContext, WorkerInput};
pub use error::{OuterError, ProblemDetail, UpstreamError, WorkerError};

use crate::application::service::operations::OperationDispatcher;
use crate::application::{
    WorkerPorts, WorkerRequestId, WorkerRequestUseCase, WorkerResponseSpiPort, jose_port,
};
use crate::domain::{HsmWorkerRequest, WorkerRequestError, WorkerResponse};
use std::sync::Arc;
use std::time::Instant;
use tracing::{error, info};

use decode::RequestDecoder;
use response::{ProcessError, ResponseBuilder};

/// Orchestrates the processing of requests from Kafka.
pub struct WorkerService {
    worker_response_spi_port: Arc<dyn WorkerResponseSpiPort + Send + Sync>,
    operation_dispatcher: OperationDispatcher,
    request_decoder: RequestDecoder,
    response_builder: ResponseBuilder,
}

impl WorkerService {
    pub fn new(jose: Arc<dyn jose_port::JosePort>, ports: WorkerPorts) -> Self {
        let operation_dispatcher = OperationDispatcher::from_dependencies(
            ports.pake,
            ports.session_key.clone(),
            ports.hsm,
        );

        let request_decoder = RequestDecoder::new(jose.clone(), ports.session_key.clone());

        let response_builder = ResponseBuilder::new(jose.clone(), ports.session_key.clone());

        Self {
            worker_response_spi_port: ports.worker_response,
            operation_dispatcher,
            request_decoder,
            response_builder,
        }
    }
}

impl WorkerRequestUseCase for WorkerService {
    /// The entry point for processing a single request from Kafka.
    /// It handles the end-to-end execution and all error reporting back to the sender.
    fn execute(
        &self,
        hsm_worker_request: HsmWorkerRequest,
    ) -> Result<WorkerRequestId, WorkerRequestError> {
        let start = Instant::now();
        let request_id = hsm_worker_request.request_id.clone();

        let response = match self.process_request(hsm_worker_request) {
            Ok(res) => res,
            Err(process_err) => {
                error!("Request {} failed: {:?}", request_id, process_err.error);
                match self
                    .response_builder
                    .build_error_response(&request_id, process_err)
                {
                    Ok(response) => response,
                    Err(build_err) => {
                        error!(
                            "Request {} failed to build error response: {:?}",
                            request_id, build_err
                        );
                        return Err(build_err);
                    }
                }
            }
        };

        self.worker_response_spi_port
            .send(response)
            .map_err(|_| WorkerRequestError::ConnectionError)?;

        info!(
            "Processed request id {} (took {} ms)",
            request_id,
            start.elapsed().as_millis(),
        );

        Ok(request_id)
    }
}

impl WorkerService {
    /// The core execution pipeline: Decode -> Dispatch -> Encode.
    fn process_request(&self, request: HsmWorkerRequest) -> Result<WorkerResponse, ProcessError> {
        // Decode
        let WorkerInput {
            operation_context,
            response_context,
        } = self
            .request_decoder
            .decode_request(request)
            .map_err(|error| ProcessError {
                error,
                context: None,
            })?;

        // Dispatch
        let operation_result = self
            .operation_dispatcher
            .dispatch(operation_context)
            .map_err(|err| ProcessError {
                error: WorkerError::Inner(err),
                context: Some(Box::new(response_context.clone())),
            })?;

        // Encode
        self.response_builder
            .encode_response(operation_result, &response_context)
            .map_err(|error| ProcessError {
                error,
                context: Some(Box::new(response_context)),
            })
    }
}
