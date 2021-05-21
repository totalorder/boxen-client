use futures::channel::mpsc::{UnboundedSender, UnboundedReceiver};
use futures::channel::mpsc;
use async_std::task;
use std::time::Duration;
use futures::{SinkExt, TryFutureExt};
use std::thread;
//use futures::{channel::mpsc, stream::{self, StreamExt, select_all}};
use futures::stream;
use futures::stream::StreamExt;
use futures::executor;

pub async fn bla() {
    let pool = executor::ThreadPool::new().unwrap();

    println!("Bla");
    let (sender1, receiver1) = mpsc::unbounded();
    let (sender2, receiver2) = mpsc::unbounded();
    let mut futs = stream::FuturesUnordered::new();

    println!("ble");
    let fut1 = send(1, 500, sender1.clone());
    // pool.spawn_ok(fut1);
    futs.push(fut1);
    let fut2 = send(2, 1000, sender1.clone());
    // pool.spawn_ok(fut2);
    futs.push(fut2);
    println!("qwe");

    let fut3 = send(3, 250, sender2.clone());
    // pool.spawn_ok(fut3);
    futs.push(fut3);
    let fut4 = send(4, 750, sender2.clone());
    // pool.spawn_ok(fut4);
    futs.push(fut4);

    let mut receivers = Vec::new();
    receivers.push(receiver1);
    receivers.push(receiver2);
    println!("ble");

    let join_handle = task::spawn(async {
        task::sleep(Duration::from_millis(700)).await;
        if true {
            panic!("w00t");
        }
        let a: u32 = 0;
        a
    });

    // let join_fut = join_handle.into_future();
    // futs.push(join_fut.into_future());

    let mut select_all = stream::select_all(receivers);
    loop {
        let message = futures::select! {
            message = select_all.next() => {
                println!("Message: {}", message.unwrap());
                message.unwrap()
            },
            _ = futs.select_next_some() => {
                println!("Thread died");
                0
            },
            complete => break
        };
        if message == 2 {
            break;
        }
    }
    // });

    // fut1.await;
    // fut2.await;
    // fut3.await;
    // fut4.await;
    println!("Done")
}

async fn send(id: u32, wait_millis: u64, mut sender: UnboundedSender<u32>) {
    task::sleep(Duration::from_millis(wait_millis)).await;
    println!("Sending {}", id);
    sender.send(id).await.unwrap();
}