use std::path::PathBuf;

/// Per-document actor.
///
/// Bootstrap stub: holds the path and a `tokio::sync::mpsc` channel.
/// The real actor (mailbox loop + dedicated thread + PDFium document
/// ownership) lands in the next Phase 1 commit. We commit the type now
/// so the `AppState` shape and the command signatures don't churn when
/// the real implementation lands.
#[allow(dead_code)] // fields are placeholders for the real actor implementation
pub struct DocumentActorHandle {
    path: PathBuf,
    tx: tokio::sync::mpsc::Sender<Message>,
}

#[derive(Debug)]
pub enum Message {
    /// Placeholder; full message set arrives with the worker loop.
    Ping,
}

impl DocumentActorHandle {
    pub fn spawn(path: PathBuf) -> Self {
        let (tx, mut rx) = tokio::sync::mpsc::channel(32);
        // We spawn a no-op consumer to keep the channel alive. This will
        // be replaced with the PDFium-owning worker loop.
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                tracing::trace!(?msg, "document actor (stub) received message");
            }
        });
        Self { path, tx }
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub async fn ping(&self) -> Result<(), tokio::sync::mpsc::error::SendError<Message>> {
        self.tx.send(Message::Ping).await
    }
}
