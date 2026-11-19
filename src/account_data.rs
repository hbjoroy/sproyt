use std::{
    future::{Future, poll_fn},
    io,
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};

use futures_util::Stream;
use serde::Serialize;
use tokio::{
    sync::{OwnedSemaphorePermit, mpsc, oneshot},
    time::Instant,
};

#[cfg(test)]
use crate::domain::{PortableUserExport, RepositoryFuture};
use crate::domain::{ProfileRepository, RepositoryError, User, UserId};

pub const ACCOUNT_EXPORT_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_CHUNK_BYTES: usize = 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum AccountExportError {
    #[error("an account export is already in progress")]
    Busy,
    #[error("account export capacity is unavailable")]
    Unavailable,
    #[error("account export exceeded its time limit")]
    Timeout,
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

pub type AccountExportFuture<'a> =
    Pin<Box<dyn Future<Output = Result<AccountExport, AccountExportError>> + Send + 'a>>;

pub(crate) trait AccountExportRepository: Send + Sync + 'static {
    fn start_user_export(
        &self,
        actor: UserId,
        deadline: tokio::time::Instant,
        permit: tokio::sync::OwnedSemaphorePermit,
    ) -> AccountExportFuture<'_>;
    #[cfg(test)]
    fn export_user_data(&self, actor: UserId) -> RepositoryFuture<'_, PortableUserExport> {
        Box::pin(async move {
            use futures_util::{TryStreamExt, pin_mut};
            let permit = std::sync::Arc::new(tokio::sync::Semaphore::new(1))
                .try_acquire_owned()
                .expect("test export permit");
            let export = self
                .start_user_export(
                    actor,
                    tokio::time::Instant::now() + ACCOUNT_EXPORT_TIMEOUT,
                    permit,
                )
                .await
                .map_err(|error| RepositoryError::Storage(error.to_string()))?;
            let stream = export.into_stream();
            pin_mut!(stream);
            let mut bytes = Vec::new();
            while let Some(chunk) = stream
                .try_next()
                .await
                .map_err(|error| RepositoryError::Storage(error.to_string()))?
            {
                bytes.extend(chunk);
            }
            serde_json::from_slice(&bytes)
                .map_err(|error| RepositoryError::Storage(error.to_string()))
        })
    }
}

#[derive(Clone)]
pub(crate) struct AccountDataService {
    profiles: Arc<dyn ProfileRepository>,
    exports: Arc<dyn AccountExportRepository>,
    export_slot: Arc<tokio::sync::Semaphore>,
}

impl AccountDataService {
    pub(crate) fn new(
        profiles: Arc<dyn ProfileRepository>,
        exports: Arc<dyn AccountExportRepository>,
    ) -> Self {
        Self {
            profiles,
            exports,
            export_slot: Arc::new(tokio::sync::Semaphore::new(1)),
        }
    }

    pub(crate) async fn start_user_export(
        &self,
        user: User,
    ) -> Result<AccountExport, AccountExportError> {
        let permit = self
            .export_slot
            .clone()
            .try_acquire_owned()
            .map_err(|_| AccountExportError::Busy)?;
        let deadline = tokio::time::Instant::now() + ACCOUNT_EXPORT_TIMEOUT;
        let actor = user.id.clone();
        tokio::time::timeout_at(deadline, self.profiles.upsert_user(user))
            .await
            .map_err(|_| AccountExportError::Timeout)??;
        self.exports
            .start_user_export(actor, deadline, permit)
            .await
    }
}

pub struct AccountExport {
    chunks: Arc<Mutex<mpsc::Receiver<Vec<u8>>>>,
    outcome: oneshot::Receiver<Result<(), RepositoryError>>,
}

impl AccountExport {
    pub(crate) fn start<F, Fut>(deadline: Instant, permit: OwnedSemaphorePermit, produce: F) -> Self
    where
        F: FnOnce(ExportWriter) -> Fut + Send + 'static,
        Fut: Future<Output = Result<(), RepositoryError>> + Send + 'static,
    {
        let (sender, receiver) = mpsc::channel(1);
        let chunks = Arc::new(Mutex::new(receiver));
        let buffered = Arc::downgrade(&chunks);
        let (finish, outcome) = oneshot::channel();
        tokio::spawn(async move {
            let _permit = permit;
            let writer = ExportWriter(sender.clone());
            let result = tokio::select! {
                biased;
                _ = sender.closed() => return,
                _ = tokio::time::sleep_until(deadline) => Err(RepositoryError::Storage("account export deadline expired".into())),
                result = async {
                    produce(writer).await?;
                    // Keep local admission until the body takes the last
                    // queued payload. Only the closing byte remains afterward.
                    sender.reserve().await.map_err(|_| RepositoryError::Storage("account export client disconnected".into()))?;
                    Ok(())
                } => result,
            };
            if result.is_err()
                && let Some(buffered) = buffered.upgrade()
            {
                let mut receiver = buffered.lock().expect("account export queue");
                receiver.close();
                while receiver.try_recv().is_ok() {}
            }
            let _ = finish.send(result);
        });
        Self { chunks, outcome }
    }

    pub fn into_stream(self) -> impl Stream<Item = Result<Vec<u8>, io::Error>> + Send {
        async_stream::try_stream! {
            while let Some(chunk) = poll_fn(|context| self.chunks.lock().expect("account export queue").poll_recv(context)).await {
                yield chunk;
            }
            match self.outcome.await {
                Ok(Ok(())) => yield b"}".to_vec(),
                _ => Err(io::Error::other("account export interrupted"))?,
            }
        }
    }
}

pub(crate) struct ExportWriter(mpsc::Sender<Vec<u8>>);

impl ExportWriter {
    pub(crate) async fn token(&self, token: &'static [u8]) -> Result<(), RepositoryError> {
        self.send(token.to_vec()).await
    }

    pub(crate) async fn value(
        &self,
        prefix: &'static [u8],
        value: &(impl Serialize + Sync),
    ) -> Result<(), RepositoryError> {
        let mut chunk = ExportChunk(prefix.to_vec());
        serde_json::to_writer(&mut chunk, value)
            .map_err(|error| RepositoryError::Storage(error.to_string()))?;
        self.send(chunk.0).await
    }

    async fn send(&self, chunk: Vec<u8>) -> Result<(), RepositoryError> {
        self.0
            .send(chunk)
            .await
            .map_err(|_| RepositoryError::Storage("account export client disconnected".into()))
    }
}

struct ExportChunk(Vec<u8>);

impl io::Write for ExportChunk {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_CHUNK_BYTES - self.0.len() {
            return Err(io::Error::other(
                "account export record exceeds the size limit",
            ));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use futures_util::{StreamExt, pin_mut};
    use tokio::sync::Semaphore;

    use super::*;

    #[test]
    fn export_path_is_independent_of_chat() {
        let service = include_str!("account_data.rs");
        let (service, _) = service.split_once("mod tests {").unwrap();
        let handler = include_str!("web/account_export.rs");
        for source in [service, handler] {
            assert!(!source.contains("ChatEngine"));
            assert!(!source.contains("ChatRepository"));
            assert!(
                !source
                    .split_whitespace()
                    .collect::<String>()
                    .contains("state.chat")
            );
        }
    }

    #[tokio::test]
    async fn unpolled_export_discards_its_queued_tail_and_releases_admission_at_deadline() {
        let slot = Arc::new(Semaphore::new(1));
        let permit = slot.clone().try_acquire_owned().unwrap();
        let (queued, written) = oneshot::channel();
        let export = AccountExport::start(
            Instant::now() + Duration::from_millis(500),
            permit,
            |writer| async move {
                writer.token(b"{\"partial\":1").await?;
                let _ = queued.send(());
                Ok(())
            },
        );
        written.await.unwrap();
        assert_eq!(slot.available_permits(), 0);
        let _reused = tokio::time::timeout(Duration::from_secs(2), slot.acquire())
            .await
            .expect("the deadline must release admission without a body poll")
            .unwrap();
        let stream = export.into_stream();
        pin_mut!(stream);
        assert!(stream.next().await.unwrap().is_err());
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn producer_failure_returns_a_body_error_after_partial_json_without_closing_it() {
        let permit = Arc::new(Semaphore::new(1)).try_acquire_owned().unwrap();
        let (fail, wait_for_failure) = oneshot::channel();
        let export = AccountExport::start(
            Instant::now() + Duration::from_secs(2),
            permit,
            |writer| async move {
                writer.token(b"{").await?;
                wait_for_failure.await.unwrap();
                Err(RepositoryError::Storage("fixture failure".into()))
            },
        );
        let stream = export.into_stream();
        pin_mut!(stream);
        assert_eq!(stream.next().await.unwrap().unwrap(), b"{");
        fail.send(()).unwrap();
        assert!(stream.next().await.unwrap().is_err());
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn disconnect_drops_the_producer_and_releases_admission() {
        struct DropSignal(Option<oneshot::Sender<()>>);
        impl Drop for DropSignal {
            fn drop(&mut self) {
                let _ = self.0.take().unwrap().send(());
            }
        }
        let slot = Arc::new(Semaphore::new(1));
        let permit = slot.clone().try_acquire_owned().unwrap();
        let (started, producing) = oneshot::channel();
        let (cancelled, producer_dropped) = oneshot::channel();
        let export = AccountExport::start(
            Instant::now() + Duration::from_secs(120),
            permit,
            |_| async move {
                let _signal = DropSignal(Some(cancelled));
                let _ = started.send(());
                std::future::pending().await
            },
        );
        producing.await.unwrap();
        drop(export);
        tokio::time::timeout(Duration::from_secs(2), producer_dropped)
            .await
            .unwrap()
            .unwrap();
        let _reused = tokio::time::timeout(Duration::from_secs(2), slot.acquire())
            .await
            .unwrap()
            .unwrap();
    }
}
