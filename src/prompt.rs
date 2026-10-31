//! Lets background connection tasks ask the UI for passwords / confirmations.

use tokio::sync::oneshot;

/// (answer, remember)
pub type Reply = oneshot::Sender<Option<(String, bool)>>;

pub struct Request {
    pub title: String,
    pub text: String,
    /// None = yes/no confirmation; Some(secret) = text input.
    pub input: Option<bool>,
    pub reply: Reply,
}

#[derive(Clone)]
pub struct Prompter {
    tx: std::sync::mpsc::Sender<Request>,
}

impl Prompter {
    pub fn new() -> (Self, std::sync::mpsc::Receiver<Request>) {
        let (tx, rx) = std::sync::mpsc::channel();
        (Self { tx }, rx)
    }

    async fn send(&self, title: &str, text: String, input: Option<bool>) -> Option<(String, bool)> {
        let (reply, rx) = oneshot::channel();
        self.tx.send(Request { title: title.into(), text, input, reply }).ok()?;
        rx.await.ok().flatten()
    }

    pub async fn ask(&self, title: &str, text: String, secret: bool) -> Option<(String, bool)> {
        self.send(title, text, Some(secret)).await
    }

    pub async fn confirm(&self, title: &str, text: String) -> bool {
        self.send(title, text, None).await.is_some()
    }
}
