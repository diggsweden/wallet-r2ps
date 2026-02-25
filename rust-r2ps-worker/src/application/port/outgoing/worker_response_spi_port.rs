use crate::domain::WorkerResponse;

pub trait WorkerResponseSpiPort {
    fn send(&self, worker_response: WorkerResponse) -> Result<(), WorkerResponseError>;
}

#[derive(Debug)]
pub enum WorkerResponseError {
    ConnectionError,
    // TODO
}
