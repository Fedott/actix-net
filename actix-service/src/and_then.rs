use futures::{Poll, TryFuture, Future};

use super::{IntoNewService, NewService, Service};
use crate::cell::Cell;
use std::pin::Pin;
use futures::task::Context;

/// Service for the `and_then` combinator, chaining a computation onto the end
/// of another service which completes successfully.
///
/// This is created by the `ServiceExt::and_then` method.
pub struct AndThen<A, B> {
    a: A,
    b: Cell<B>,
}

impl<A, B> AndThen<A, B> {
    /// Create new `AndThen` combinator
    pub fn new(a: A, b: B) -> Self
    where
        A: Service,
        B: Service<Request = A::Response, Error = A::Error>,
    {
        Self { a, b: Cell::new(b) }
    }
}

impl<A, B> Clone for AndThen<A, B>
where
    A: Clone,
{
    fn clone(&self) -> Self {
        AndThen {
            a: self.a.clone(),
            b: self.b.clone(),
        }
    }
}

impl<A, B> Service for AndThen<A, B>
where
    A: Service,
    B: Service<Request = A::Response, Error = A::Error>,
{
    type Request = A::Request;
    type Response = B::Response;
    type Error = A::Error;
    type Future = AndThenFuture<A, B>;

    fn poll_ready(&mut self) -> Poll<Result<(), Self::Error>> {
        let not_ready = !self.a.poll_ready().is_ready();
        if !self.b.get_mut().poll_ready().is_ready() || not_ready {
            Poll::Pending
        } else {
            Poll::Ready(Ok(()))
        }
    }

    fn call(&mut self, req: A::Request) -> Self::Future {
        AndThenFuture::new(self.a.call(req), self.b.clone())
    }
}

pub struct AndThenFuture<A, B>
where
    A: Service,
    B: Service<Request = A::Response, Error = A::Error>,
{
    b: Cell<B>,
    fut_b: Option<B::Future>,
    fut_a: Option<A::Future>,
}

impl<A, B> AndThenFuture<A, B>
where
    A: Service,
    B: Service<Request = A::Response, Error = A::Error>,
{
    fn new(a: A::Future, b: Cell<B>) -> Self {
        AndThenFuture {
            b,
            fut_a: Some(a),
            fut_b: None,
        }
    }
}

impl<A, B> Future for AndThenFuture<A, B>
    where
        A: Service,
        B: Service<Request = A::Response, Error = A::Error>,
{
    type Output = Result<B::Response, A::Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(ref mut fut) = self.fut_b {
            return fut.try_poll();
        }

        match self.fut_a.as_mut().expect("Bug in actix-service").try_poll() {
            Poll::Ready(Ok(resp)) => {
                let _ = self.fut_a.take();
                self.fut_b = Some(self.b.get_mut().call(resp));
                self.try_poll()
            }
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
        }
    }
}

/// `AndThenNewService` new service combinator
pub struct AndThenNewService<A, B>
where
    A: NewService,
    B: NewService,
{
    a: A,
    b: B,
}

impl<A, B> AndThenNewService<A, B>
where
    A: NewService,
    B: NewService<
        Config = A::Config,
        Request = A::Response,
        Error = A::Error,
        InitError = A::InitError,
    >,
{
    /// Create new `AndThen` combinator
    pub fn new<F: IntoNewService<B>>(a: A, f: F) -> Self {
        Self {
            a,
            b: f.into_new_service(),
        }
    }
}

impl<A, B> NewService for AndThenNewService<A, B>
where
    A: NewService,
    B: NewService<
        Config = A::Config,
        Request = A::Response,
        Error = A::Error,
        InitError = A::InitError,
    >,
{
    type Request = A::Request;
    type Response = B::Response;
    type Error = A::Error;

    type Config = A::Config;
    type Service = AndThen<A::Service, B::Service>;
    type InitError = A::InitError;
    type Future = AndThenNewServiceFuture<A, B>;

    fn new_service(&self, cfg: &A::Config) -> Self::Future {
        AndThenNewServiceFuture::new(self.a.new_service(cfg), self.b.new_service(cfg))
    }
}

impl<A, B> Clone for AndThenNewService<A, B>
where
    A: NewService + Clone,
    B: NewService + Clone,
{
    fn clone(&self) -> Self {
        Self {
            a: self.a.clone(),
            b: self.b.clone(),
        }
    }
}

pub struct AndThenNewServiceFuture<A, B>
where
    A: NewService,
    B: NewService<Request = A::Response>,
{
    fut_b: B::Future,
    fut_a: A::Future,
    a: Option<A::Service>,
    b: Option<B::Service>,
}

impl<A, B> AndThenNewServiceFuture<A, B>
where
    A: NewService,
    B: NewService<Request = A::Response>,
{
    fn new(fut_a: A::Future, fut_b: B::Future) -> Self {
        AndThenNewServiceFuture {
            fut_a,
            fut_b,
            a: None,
            b: None,
        }
    }
}

impl<A, B> Future for AndThenNewServiceFuture<A, B>
where
    A: NewService,
    B: NewService<Request = A::Response, Error = A::Error, InitError = A::InitError>,
{
    type Output = Result<AndThen<A::Service, B::Service>, A::InitError>;

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Self::Output> {
        if self.a.is_none() {
            if let Poll::Ready(service) = self.fut_a.try_poll() {
                self.a = Some(service?);
            }
        }

        if self.b.is_none() {
            if let Poll::Ready(service) = self.fut_b.try_poll() {
                self.b = Some(service?);
            }
        }

        if self.a.is_some() && self.b.is_some() {
            Poll::Ready(Ok(AndThen::new(
                self.a.take().unwrap(),
                self.b.take().unwrap(),
            )))
        } else {
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod tests {
    use futures::future::{ok, Ready};
    use futures::{Poll};
    use std::cell::Cell;
    use std::rc::Rc;

    use super::*;
    use crate::{NewService, Service, ServiceExt};

    struct Srv1(Rc<Cell<usize>>);
    impl Service for Srv1 {
        type Request = &'static str;
        type Response = &'static str;
        type Error = ();
        type Future = Ready<Result<Self::Response, ()>>;

        fn poll_ready(&mut self) -> Poll<Result<(), Self::Error>> {
            self.0.set(self.0.get() + 1);
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, req: &'static str) -> Self::Future {
            ok(req)
        }
    }

    #[derive(Clone)]
    struct Srv2(Rc<Cell<usize>>);

    impl Service for Srv2 {
        type Request = &'static str;
        type Response = (&'static str, &'static str);
        type Error = ();
        type Future = Ready<Result<Self::Response, ()>>;

        fn poll_ready(&mut self) -> Poll<Result<(), Self::Error>> {
            self.0.set(self.0.get() + 1);
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, req: &'static str) -> Self::Future {
            ok((req, "srv2"))
        }
    }

    #[test]
    fn test_poll_ready() {
        let cnt = Rc::new(Cell::new(0));
        let mut srv = Srv1(cnt.clone()).and_then(Srv2(cnt.clone()));
        let res = srv.poll_ready();
        assert_eq!(res, Poll::Ready(Ok(())));
        assert_eq!(cnt.get(), 2);
    }

    #[test]
    fn test_call() {
        let cnt = Rc::new(Cell::new(0));
        let mut srv = Srv1(cnt.clone()).and_then(Srv2(cnt));
        let res = srv.call("srv1").try_poll();
        assert_eq!(res, Poll::Ready(Ok(("srv1", "srv2"))));
    }

    #[test]
    fn test_new_service() {
        let cnt = Rc::new(Cell::new(0));
        let cnt2 = cnt.clone();
        let blank = move || Ok::<_, ()>(Srv1(cnt2.clone()));
        let new_srv = blank
            .into_new_service()
            .and_then(move || Ok(Srv2(cnt.clone())));
        if let Poll::Ready(Ok(mut srv)) = new_srv.new_service(&()).try_poll() {
            let res = srv.call("srv1").poll();
            assert_eq!(res, Poll::Ready(Ok(("srv1", "srv2"))));
        } else {
            panic!()
        }
    }
}
