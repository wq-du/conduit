use std::mem;

use futures::{Async, Future, Poll, Stream};
use futures::future::Shared;
use futures::sync::{mpsc, oneshot};

/// Creates a drain channel.
///
/// The `Signal` is used to start a drain, and the `Watch` will be notified
/// when a drain is signaled.
pub fn channel() -> (Signal, Watch) {
    let (tx, rx) = oneshot::channel();
    let (drained_tx, drained_rx) = mpsc::channel(0);
    (
        Signal {
            drained_rx,
            tx,
        },
        Watch {
            drained_tx,
            rx: rx.shared(),
        },
    )
}

/// Send a drain command to all watchers.
///
/// When a drain is started, this returns a `Drained` future which resolves
/// when all `Watch`ers have been dropped.
#[derive(Debug)]
pub struct Signal {
    drained_rx: mpsc::Receiver<Never>,
    tx: oneshot::Sender<()>,
}

/// Watch for a drain command.
///
/// This wraps another future and callback to be called when drain is triggered.
#[derive(Clone, Debug)]
pub struct Watch {
    drained_tx: mpsc::Sender<Never>,
    rx: Shared<oneshot::Receiver<()>>,
}

/// The wrapped watching `Future`.
#[derive(Debug)]
pub struct Watching<A, F> {
    future: A,
    state: State<F>,
    watch: Watch,
}

#[derive(Debug)]
enum State<F> {
    Watch(F),
    Draining,
}

//TODO: in Rust 1.26, replace this with `!`.
#[derive(Debug)]
enum Never {}

/// A future that resolves when all `Watch`ers have been dropped (drained).
pub struct Drained {
    drained_rx: mpsc::Receiver<Never>,
}

// ===== impl Signal =====

impl Signal {
    /// Start the draining process.
    ///
    /// A signal is sent to all futures watching for the signal. A new future
    /// is returned from this method that resolves when all watchers have
    /// completed.
    pub fn drain(self) -> Drained {
        let _ = self.tx.send(());
        Drained {
            drained_rx: self.drained_rx,
        }
    }
}

// ===== impl Watch =====

impl Watch {
    /// Wrap a future and a callback that is triggered when drain is received.
    ///
    /// The callback receives a mutable reference to the original future, and
    /// should be used to trigger any shutdown process for it.
    pub fn watch<A, F>(self, future: A, on_drain: F) -> Watching<A, F>
    where
        A: Future,
        F: FnOnce(&mut A),
    {
        Watching {
            future,
            state: State::Watch(on_drain),
            watch: self,
        }
    }
}

// ===== impl Watching =====

impl<A, F> Future for Watching<A, F>
where
    A: Future,
    F: FnOnce(&mut A),
{
    type Item = A::Item;
    type Error = A::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            match mem::replace(&mut self.state, State::Draining) {
                State::Watch(on_drain) => {
                    match self.watch.rx.poll() {
                        Ok(Async::Ready(_)) | Err(_) => {
                            // Drain has been triggered!
                            on_drain(&mut self.future);
                        },
                        Ok(Async::NotReady) => {
                            self.state = State::Watch(on_drain);
                            return self.future.poll();
                        }
                    }
                },
                State::Draining => {
                    return self.future.poll();
                },
            }
        }
    }
}

// ===== impl Drained =====

impl Future for Drained {
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match try_ready!(self.drained_rx.poll()) {
            Some(never) => match never {},
            None => Ok(Async::Ready(())),
        }
    }
}

/*
#[cfg(test)]
mod tests {
    use futures::future;
    use super::*;

    #[test]
    fn watch() {
        future::lazy(|| {

            let (tx, rx) = channel();

        }).wait().unwrap();

    }
}
*/
