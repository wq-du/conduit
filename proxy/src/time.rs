//! Timeouts and abstraction over timer implementations.
#![deny(missing_docs)]
use std::{fmt, io};
use std::error::Error;
use std::time::{Duration, Instant};
use std::sync::Arc;

use futures::{Future, Poll, Async};
use tokio_connect::Connect;
use tokio_io;
use tokio_timer;
use tower::Service;

/// Abstraction over the interface required for a timer.
///
/// This trait exists primarily so that we can provide implementations for
/// both `tokio_timer` and a mock timer for tests.
pub trait Timer: Clone + Sized {
    /// The type of the future returned by `sleep`.
    type Sleep: Future<Item=(), Error=Self::Error>;
    /// Error type for the `Sleep` future.
    ///
    /// This error will indicate an error *in the timer*, not that
    /// a timeout was exceeded.
    type Error;

    /// Returns a future that completes after the given duration.
    fn sleep(&self, duration: Duration) -> Self::Sleep;

    /// Returns the current time.
    ///
    /// This takes `&self` primarily for the mock timer implementation.
    fn now(&self) -> Instant;

    /// Returns a `Timeout` service using this timer.
    fn timeout<U>(&self, upstream: U, duration: Duration)
                     -> Timeout<U, Self>
    {
        Timeout {
            upstream,
            timer: self.clone(),
            duration,
            description: None,
        }
    }

    /// Returns a `NewTimeout` using this timer.
    fn new_timeout(&self, duration: Duration) -> NewTimeout<Self> {
        NewTimeout {
            timer: self.clone(),
            duration,
            description: None,
        }
    }
}

/// Applies a timeout to requests.
#[derive(Clone, Debug)]
pub struct Timeout<S, T> {
    upstream: S,
    timer: T,
    duration: Duration,
    description: Option<Arc<String>>,
}

/// Wraps services with a preset timeout.
#[derive(Clone, Debug)]
pub struct NewTimeout<T> {
    timer: T,
    duration: Duration,
    description: Option<Arc<String>>,
}

/// Errors produced by `Timeout`.
#[derive(Clone, Debug)]
pub enum TimeoutError<U, T> {
    /// The inner service produced an error
    Upstream(U),

    /// The timer produced an error
    Timer(T),

    /// The request did not complete within the specified timeout.
    Timeout {
        /// The duration exceeded by the timed-out request.
        after: Duration,
        /// An optional description naming the request that timed out.
        description: Option<Arc<String>>,
    },

}

/// `Timeout` inner future
#[derive(Debug)]
pub struct TimeoutFuture<F, S> {
    inner: F,
    sleep: S,
    description: Option<Arc<String>>,
    after: Duration
}

// ===== impl Timer =====

impl Timer for tokio_timer::Timer {
    type Sleep = tokio_timer::Sleep;
    type Error = tokio_timer::TimerError;

    /// Returns a future that completes after the given duration.
    fn sleep(&self, duration: Duration) -> Self::Sleep {
        self.sleep(duration)
    }

    /// Returns the current time.
    ///
    /// This takes `&self` primarily for the mock timer implementation.
    fn now(&self) -> Instant {
        Instant::now()
    }
}

// ===== impl Timeout =====

impl<S, T> Timeout<S, T> {

    /// Add a description to this timeout.
    ///
    /// The description will be used primarily for adding context
    /// to the error message for the timeout's `TimeoutError`.
    pub fn with_description<I>(self, description: I) -> Self
    where
        I: Into<String>,
    {
        let description = Some(Arc::new(description.into()));
        Timeout {
            timer: self.timer,
            upstream: self.upstream,
            duration: self.duration,
            description,
        }
    }

}

impl<S, T> Timeout<S, T>
where
    T: Timer,
{
    #[inline]
    fn future<F>(&self, inner: F) -> TimeoutFuture<F, T::Sleep> {
        let description = self.description.as_ref().map(Arc::clone);
        let sleep = self.timer.sleep(self.duration);
        let after = self.duration;
        TimeoutFuture {
            inner,
            sleep,
            description,
            after,
        }
    }
}

impl<S, T> Service for Timeout<S, T>
where
    S: Service,
    T: Timer,
{
    type Request = S::Request;
    type Response = S::Response;
    type Error = TimeoutError<S::Error, T::Error>;
    type Future = TimeoutFuture<S::Future, T::Sleep>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        self.upstream.poll_ready()
            .map_err(TimeoutError::Upstream)
    }

    fn call(&mut self, request: Self::Request) -> Self::Future {
        let inner = self.upstream.call(request);
        self.future(inner)
    }
}

impl<C, T> Connect for Timeout<C, T>
where
    C: Connect,
    T: Timer,
{
    type Connected = C::Connected;
    type Error = TimeoutError<C::Error, T::Error>;
    type Future = TimeoutFuture<C::Future, T::Sleep>;

    fn connect(&self) -> Self::Future {
        let inner = self.upstream.connect();
        self.future(inner)
    }
}


impl<C, T> io::Read for Timeout<C, T>
where
    C: io::Read,
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.upstream.read(buf)
    }
}

impl<C, T> io::Write for Timeout<C, T>
where
    C: io::Write,
{
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.upstream.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.upstream.flush()
    }
}

impl<C, T> tokio_io::AsyncRead for Timeout<C, T>
where
    C: tokio_io::AsyncRead,
{
    unsafe fn prepare_uninitialized_buffer(&self, buf: &mut [u8]) -> bool {
        self.upstream.prepare_uninitialized_buffer(buf)
    }
}

impl<C, T> tokio_io::AsyncWrite for Timeout<C, T>
where
    C: tokio_io::AsyncWrite,
{
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        self.upstream.shutdown()
    }
}


// ===== impl NewTimeout =====

impl<T> NewTimeout<T> {

    /// Add a description to the returned timeout.
    ///
    /// The description will be used primarily for adding context
    /// to the error message for the timeout's `TimeoutError`.
    pub fn with_description<I>(self, description: I) -> Self
    where
        I: Into<String>,
    {
        let description = Some(Arc::new(description.into()));
        NewTimeout {
            timer: self.timer,
            duration: self.duration,
            description,
        }
    }

    /// Apply the timeout to the given `upstream` service, creating a
    /// `Timeout` service.
    pub fn apply_to<U>(&self, upstream: U) -> Timeout<U, T>
    where
        T: Clone,
    {
        let description = self.description.as_ref().map(Arc::clone);
        Timeout {
            upstream,
            timer: self.timer.clone(),
            duration: self.duration,
            description,

        }
    }

    /// Borrow the `Timer` backing this `NewTimeout`.
    #[inline]
    #[allow(dead_code)]
    pub fn timer(&self) -> &T {
        &self.timer
    }

}

// ===== impl TimeoutFuture =====

impl<F, S> TimeoutFuture<F, S> {

    #[inline]
    fn timeout_error<E, T>(&self) -> TimeoutError<E, T> {
        let description = self.description.as_ref().map(Arc::clone);
        TimeoutError::Timeout {
            after: self.after,
            description,
        }
    }

}

impl<F, S> Future for TimeoutFuture<F, S>
where
    F: Future,
    S: Future,
{
    type Item = F::Item;
    type Error = TimeoutError<F::Error, S::Error>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        // First, try polling the future
        match self.inner.poll() {
            Ok(Async::Ready(v)) => return Ok(Async::Ready(v)),
            Ok(Async::NotReady) => {}
            Err(e) => return Err(TimeoutError::Upstream(e)),
        }

        // Now check the sleep
        match self.sleep.poll() {
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Ok(Async::Ready(_)) => Err(self.timeout_error()),
            Err(e) => Err(TimeoutError::Timer(e)),
        }
    }
}

// ===== impl TimeoutError =====

impl<U, T> fmt::Display for TimeoutError<U, T>
where
    U: fmt::Display,
    T: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            TimeoutError::Timeout { ref after, description: Some(ref what) } =>
                write!(f, "{} timed out after {:?}", what, after),
            TimeoutError::Timeout { ref after, description: None } =>
                write!(f, "operation timed out after {:?}", after),
            TimeoutError::Timer(ref err) => fmt::Display::fmt(err, f),
            TimeoutError::Upstream(ref err) => fmt::Display::fmt(err, f),
        }
    }
}

impl<U, T> Error for TimeoutError<U, T>
where
    U: Error,
    T: Error,
{
    fn cause(&self) -> Option<&Error> {
        match *self {
            TimeoutError::Upstream(ref err) => Some(err),
            TimeoutError::Timer(ref err) => Some(err),
            _ => None,
        }
    }

    fn description(&self) -> &str {
        match *self {
            TimeoutError::Timeout { .. } => "operation timed out",
            TimeoutError::Upstream(ref err) => err.description(),
            TimeoutError::Timer(ref err) => err.description(),
        }
    }
}

