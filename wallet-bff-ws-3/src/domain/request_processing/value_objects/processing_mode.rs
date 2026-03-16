/// Determines how a service request is processed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessingMode {
    /// Wait for the HSM worker response before returning to the client.
    Synchronous,
    /// Return immediately with a pending status; client polls for the result.
    Asynchronous,
}

impl ProcessingMode {
    pub fn is_sync(&self) -> bool {
        matches!(self, Self::Synchronous)
    }
}
