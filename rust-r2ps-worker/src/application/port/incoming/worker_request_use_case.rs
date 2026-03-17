use crate::domain::{HsmWorkerRequest, StateInitCommandDto, WorkerRequestError};

pub trait WorkerRequestUseCase {
    fn execute(
        &self,
        hsm_worker_request: HsmWorkerRequest,
    ) -> Result<WorkerRequestId, WorkerRequestError>;

    fn execute_state_init(
        &self,
        command: StateInitCommandDto,
    ) -> Result<WorkerRequestId, WorkerRequestError>;
}

pub type WorkerRequestId = String;
