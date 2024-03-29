use std::thread;

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::oneshot::{error::TryRecvError, self};

pub struct Task<T> {
    receiver: oneshot::Receiver<Result<T>>,
    panic_on_drop: bool,
}

impl<T: Send + 'static> Task<T> {
    pub fn new(runner: impl TaskRunner<T> + Send + 'static, panic_on_drop: bool) -> Self {
        let (sender, receiver) = oneshot::channel();

        tokio::spawn(async move {
            let result = runner.run().await;
            if sender.send(result).is_err() {
                panic!("Failed to send result to task receiver");
            }
        });

        Self { receiver, panic_on_drop }
    }

    pub fn poll(&mut self) -> Result<Option<T>> {
        match self.receiver.try_recv() {
            // Task completed successfully
            Ok(Ok(result)) => Ok(Some(result)),
            // Task completed with an error
            Ok(Err(err))   => Err(err),
            // Task is still running
            Err(TryRecvError::Empty) => Ok(None),
            // Task panicked or otherwise dropped
            Err(TryRecvError::Closed) => Err(anyhow::anyhow!("Task receiver disconnected")),
        }
    }
}

impl<T> Drop for Task<T> {
    fn drop(&mut self) {
        if self.panic_on_drop && !matches!(self.receiver.try_recv(), Err(TryRecvError::Closed)) {
            if !thread::panicking() {
                panic!("Dropped Task handle without waiting for task to complete");
            }
        }
    }
}

#[async_trait]
pub trait TaskRunner<T> {
    async fn run(self) -> Result<T>;
}
