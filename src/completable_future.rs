use async_std::task;
use futures::{channel, SinkExt, StreamExt};

pub struct CompletableFuture<T> {
    receiver: channel::mpsc::Receiver<T>,
}

impl<T: 'static + Send + Clone> CompletableFuture<T> {
    pub fn new() -> (CompletableFuture<T>, FutureCompleter<T>) {
        let (sender, receiver) = channel::mpsc::channel(1);

        let completable_future = CompletableFuture { receiver };
        let future_completer = FutureCompleter { sender };

        (completable_future, future_completer)
    }

    pub async fn run(self) -> T {
        let (result, _receiver) = self.receiver.into_future().await;
        result.unwrap()
    }
}

#[derive(Clone)]
pub struct FutureCompleter<T: 'static + Send + Clone> {
    sender: channel::mpsc::Sender<T>
}

impl<T: 'static + Send + Clone> FutureCompleter<T> {
    pub fn complete(&self, result: T) {
        let mut sender_clone = self.sender.clone();
        task::spawn(async move {
            sender_clone.send(result).await.unwrap();
        });
    }

    pub async fn complete_async(&self, result: T) {
        self.sender.clone().send(result).await.unwrap();
    }
}
