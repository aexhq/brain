use brain_protocol::{JournalId, OperationId, operation_id};

#[derive(Clone, Debug)]
pub struct OperationAllocator {
    journal_id: JournalId,
    next_position: u64,
}

impl OperationAllocator {
    pub fn new(journal_id: JournalId, next_position: u64) -> Self {
        Self {
            journal_id,
            next_position,
        }
    }
    pub fn allocate(&mut self) -> OperationId {
        let id = operation_id(&self.journal_id, self.next_position);
        self.next_position += 1;
        id
    }
}
