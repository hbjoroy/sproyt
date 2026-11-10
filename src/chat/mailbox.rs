//! One admission path for every actor command, including notifications and sweeps.
use super::Command;
use crate::operations::capacity::{self, CapacityMetrics, ChatGuard, ChatOperation, ChatPhase};
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Clone)]
pub(super) struct Mailbox {
    sender: mpsc::Sender<Envelope>,
    metrics: Arc<CapacityMetrics>,
}
pub(super) struct Envelope {
    pub command: Command,
    queued: ChatGuard,
    operation: ChatOperation,
    metrics: Arc<CapacityMetrics>,
}
impl Envelope {
    pub fn begin(self) -> (Command, ChatGuard) {
        self.queued.finish();
        let execution = self.metrics.chat(self.operation, ChatPhase::Execution);
        (self.command, execution)
    }
}
impl Mailbox {
    pub(super) fn channel(capacity: usize) -> (Self, mpsc::Receiver<Envelope>) {
        Self::with_metrics(capacity, capacity::shared())
    }
    fn with_metrics(
        capacity: usize,
        metrics: Arc<CapacityMetrics>,
    ) -> (Self, mpsc::Receiver<Envelope>) {
        let (sender, receiver) = mpsc::channel(capacity);
        (Self { sender, metrics }, receiver)
    }
    pub(super) async fn send(&self, command: Command) -> Result<(), ()> {
        let operation = command.operation();
        let admission = self.metrics.chat(operation, ChatPhase::Admission);
        let permit = match self.sender.reserve().await {
            Ok(permit) => permit,
            Err(_) => {
                admission.closed();
                return Err(());
            }
        };
        admission.finish();
        // No await between the timestamp/gauge increment and publication. The
        // receiver can never decrement a gauge before its producer increments it.
        let queued = self.metrics.chat(operation, ChatPhase::Queued);
        permit.send(Envelope {
            command,
            queued,
            operation,
            metrics: self.metrics.clone(),
        });
        Ok(())
    }
}
impl Command {
    fn operation(&self) -> ChatOperation {
        match self {
            Self::ChannelCreated { .. } => ChatOperation::ChannelCreated,
            Self::Subscribe { .. } => ChatOperation::Subscribe,
            Self::Leave { .. } => ChatOperation::Leave,
            Self::SendMessage { .. } => ChatOperation::Send,
            Self::ToggleReaction { .. } => ChatOperation::Reaction,
            Self::EditMessage { .. } => ChatOperation::Edit,
            Self::DeleteMessage { .. } => ChatOperation::Delete,
            Self::ExternalMessage { .. } => ChatOperation::ExternalMessage,
            Self::ExternalReaction { .. } => ChatOperation::ExternalReaction,
            Self::ExternalUpdate { .. } => ChatOperation::ExternalUpdate,
            Self::ExternalChannel { .. } => ChatOperation::ExternalChannel,
            Self::ExternalPresence { .. } => ChatOperation::ExternalPresence,
            Self::ExpirePresence => ChatOperation::ExpirePresence,
        }
    }
}

#[cfg(test)]
mod tests;
