#![allow(unused)]

extern crate bytes;
pub extern crate conduit_proxy_controller_grpc;
extern crate conduit_proxy;
pub extern crate convert;
extern crate futures;
extern crate h2;
pub extern crate http;
extern crate hyper;
extern crate prost;
extern crate tokio_connect;
extern crate tokio_core;
pub extern crate tokio_io;
extern crate tower;
extern crate tower_h2;
extern crate log;
pub extern crate env_logger;

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
pub use std::time::Duration;

use self::bytes::{BigEndian, BytesMut};
pub use self::bytes::Bytes;
pub use self::conduit_proxy::*;
pub use self::futures::*;
use self::futures::sync::{mpsc, oneshot};
pub use self::http::{HeaderMap, Request, Response, StatusCode};
use self::http::header::HeaderValue;
use self::tokio_connect::Connect;
use self::tokio_core::net::{TcpListener, TcpStream};
use self::tokio_core::reactor::{Core, Handle};
use self::tower::{NewService, Service};
use self::tower_h2::{Body, RecvBody};

pub mod client;
pub mod controller;
pub mod proxy;
pub mod server;
mod tcp;

pub fn shutdown_signal() -> (Shutdown, ShutdownRx) {
    let (tx, rx) = oneshot::channel();
    (Shutdown { tx }, Box::new(rx.then(|_| Ok(()))))
}

pub struct Shutdown {
    tx: oneshot::Sender<()>,
}

impl Shutdown {
    pub fn signal(self) {
        // a drop is enough
    }
}

pub type ShutdownRx = Box<Future<Item=(), Error=()> + Send>;

/// A channel used to signal when a Client's related connection is running or closed.
pub fn running() -> (mpsc::Sender<()>, Running) {
    let (tx, rx) = mpsc::channel(0);
    let rx = Box::new(rx.for_each(|()| -> Result<(), ()> {
        panic!("running shouldn't ever receive")
    }));
    (tx, rx)
}

pub type Running = Box<Future<Item=(), Error=()> + Send>;

struct RecvBodyStream(tower_h2::RecvBody);

impl Stream for RecvBodyStream {
    type Item = Bytes;
    type Error = h2::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let data = try_ready!(self.0.poll_data());
        Ok(Async::Ready(data.map(From::from)))
    }
}

pub fn s(bytes: &[u8]) -> &str {
    ::std::str::from_utf8(bytes.as_ref()).unwrap()
}
