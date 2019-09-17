use std::marker::PhantomData;

use futures::{Poll, TryFuture, Future};

use super::{IntoNewService, IntoService, NewService, Service};
use crate::cell::Cell;
use std::pin::Pin;
use futures::task::Context;
use crate::IntoTryFuture;

/// `Apply` service combinator
pub struct AndThenApply<A, B, F, Out>
where
    A: Service,
    B: Service<Error = A::Error>,
    F: FnMut(A::Response, &mut B) -> Out,
    Out: IntoTryFuture,
    Out::Error: Into<A::Error>,
{
    a: A,
    b: Cell<B>,
    f: Cell<F>,
    r: PhantomData<(Out,)>,
}

impl<A, B, F, Out> AndThenApply<A, B, F, Out>
where
    A: Service,
    B: Service<Error = A::Error>,
    F: FnMut(A::Response, &mut B) -> Out,
    Out: IntoTryFuture,
    Out::Error: Into<A::Error>,
{
    /// Create new `Apply` combinator
    pub fn new<A1: IntoService<A>, B1: IntoService<B>>(a: A1, b: B1, f: F) -> Self {
        Self {
            f: Cell::new(f),
            a: a.into_service(),
            b: Cell::new(b.into_service()),
            r: PhantomData,
        }
    }
}

impl<A, B, F, Out> Clone for AndThenApply<A, B, F, Out>
where
    A: Service + Clone,
    B: Service<Error = A::Error>,
    F: FnMut(A::Response, &mut B) -> Out,
    Out: IntoTryFuture,
    Out::Error: Into<A::Error>,
{
    fn clone(&self) -> Self {
        AndThenApply {
            a: self.a.clone(),
            b: self.b.clone(),
            f: self.f.clone(),
            r: PhantomData,
        }
    }
}

impl<A, B, F, Out> Service for AndThenApply<A, B, F, Out>
where
    A: Service,
    B: Service<Error = A::Error>,
    F: FnMut(A::Response, &mut B) -> Out,
    Out: IntoTryFuture,
    Out::Error: Into<A::Error>,
{
    type Request = A::Request;
    type Response = Out::Ok;
    type Error = A::Error;
    type Future = AndThenApplyFuture<A, B, F, Out>;

    fn poll_ready(&mut self) -> Poll<Result<(), Self::Error>> {
        let not_ready = self.a.poll_ready()?.is_not_ready();
        if self.b.get_mut().poll_ready()?.is_not_ready() || not_ready {
            Poll::Pending
        } else {
            Poll::Ready(Ok(()))
        }
    }

    fn call(&mut self, req: A::Request) -> Self::Future {
        AndThenApplyFuture {
            b: self.b.clone(),
            f: self.f.clone(),
            fut_b: None,
            fut_a: Some(self.a.call(req)),
        }
    }
}

pub struct AndThenApplyFuture<A, B, F, Out>
where
    A: Service,
    B: Service<Error = A::Error>,
    F: FnMut(A::Response, &mut B) -> Out,
    Out: IntoTryFuture,
    Out::Error: Into<A::Error>,
{
    b: Cell<B>,
    f: Cell<F>,
    fut_a: Option<A::Future>,
    fut_b: Option<Out::Future>,
}

impl<A, B, F, Out> Future for AndThenApplyFuture<A, B, F, Out>
where
    A: Service,
    B: Service<Error = A::Error>,
    F: FnMut(A::Response, &mut B) -> Out,
    Out: IntoTryFuture,
    Out::Error: Into<A::Error>,
{
    type Output = Result<Out::Ok, A::Error>;

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Self::Output> {
        if let Some(ref mut fut) = self.fut_b {
            return fut.try_poll().map_err(|e| e.into());
        }

        match self.fut_a.as_mut().expect("Bug in actix-service").try_poll() {
            Poll::Ready(Ok(resp)) => {
                let _ = self.fut_a.take();
                self.fut_b =
                    Some((&mut *self.f.get_mut())(resp, self.b.get_mut()).into_try_future());
                self.poll()
            }
            Poll::Pending => Poll::Pending,
            Err(err) => Err(err),
        }
    }
}

/// `ApplyNewService` new service combinator
pub struct AndThenApplyNewService<A, B, F, Out> {
    a: A,
    b: B,
    f: Cell<F>,
    r: PhantomData<Out>,
}

impl<A, B, F, Out> AndThenApplyNewService<A, B, F, Out>
where
    A: NewService,
    B: NewService<Config = A::Config, Error = A::Error, InitError = A::InitError>,
    F: FnMut(A::Response, &mut B::Service) -> Out,
    Out: IntoTryFuture,
    Out::Error: Into<A::Error>,
{
    /// Create new `ApplyNewService` new service instance
    pub fn new<A1: IntoNewService<A>, B1: IntoNewService<B>>(a: A1, b: B1, f: F) -> Self {
        Self {
            f: Cell::new(f),
            a: a.into_new_service(),
            b: b.into_new_service(),
            r: PhantomData,
        }
    }
}

impl<A, B, F, Out> Clone for AndThenApplyNewService<A, B, F, Out>
where
    A: Clone,
    B: Clone,
{
    fn clone(&self) -> Self {
        Self {
            a: self.a.clone(),
            b: self.b.clone(),
            f: self.f.clone(),
            r: PhantomData,
        }
    }
}

impl<A, B, F, Out> NewService for AndThenApplyNewService<A, B, F, Out>
where
    A: NewService,
    B: NewService<Config = A::Config, Error = A::Error, InitError = A::InitError>,
    F: FnMut(A::Response, &mut B::Service) -> Out,
    Out: IntoTryFuture,
    Out::Error: Into<A::Error>,
{
    type Request = A::Request;
    type Response = Out::Ok;
    type Error = A::Error;
    type Service = AndThenApply<A::Service, B::Service, F, Out>;
    type Config = A::Config;
    type InitError = A::InitError;
    type Future = AndThenApplyNewServiceFuture<A, B, F, Out>;

    fn new_service(&self, cfg: &A::Config) -> Self::Future {
        AndThenApplyNewServiceFuture {
            a: None,
            b: None,
            f: self.f.clone(),
            fut_a: self.a.new_service(cfg).into_try_future(),
            fut_b: self.b.new_service(cfg).into_try_future(),
        }
    }
}

pub struct AndThenApplyNewServiceFuture<A, B, F, Out>
where
    A: NewService,
    B: NewService<Error = A::Error, InitError = A::InitError>,
    F: FnMut(A::Response, &mut B::Service) -> Out,
    Out: IntoTryFuture,
    Out::Error: Into<A::Error>,
{
    fut_b: B::Future,
    fut_a: A::Future,
    f: Cell<F>,
    a: Option<A::Service>,
    b: Option<B::Service>,
}

impl<A, B, F, Out> Future for AndThenApplyNewServiceFuture<A, B, F, Out>
where
    A: NewService,
    B: NewService<Error = A::Error, InitError = A::InitError>,
    F: FnMut(A::Response, &mut B::Service) -> Out,
    Out: IntoTryFuture,
    Out::Error: Into<A::Error>,
{
    type Output = Result<AndThenApply<A::Service, B::Service, F, Out>, A::InitError>;

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
            Poll::Ready(Ok(AndThenApply {
                f: self.f.clone(),
                a: self.a.take().unwrap(),
                b: Cell::new(self.b.take().unwrap()),
                r: PhantomData,
            }))
        } else {
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod tests {
    use futures::future::{ok, Ready};
    use futures::{Poll};

    use crate::blank::{Blank, BlankNewService};
    use crate::{NewService, Service, ServiceExt};

    type FutureResult<I, E> = Ready<Result<I, E>>;

    #[derive(Clone)]
    struct Srv;
    impl Service for Srv {
        type Request = ();
        type Response = ();
        type Error = ();
        type Future = FutureResult<(), ()>;

        fn poll_ready(&mut self) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, _: ()) -> Self::Future {
            ok(())
        }
    }

    #[test]
    fn test_call() {
        let mut srv = Blank::new().apply_fn(Srv, |req: &'static str, srv| {
            srv.call(()).map(move |res| (req, res))
        });
        assert!(srv.poll_ready().is_ready());
        let res = srv.call("srv").try_poll();
        assert!(res.is_ready());
        assert_eq!(res, Poll::Ready(Ok(("srv", ()))));
    }

    #[test]
    fn test_new_service() {
        let new_srv = BlankNewService::new_unit().apply_fn(
            || Ok(Srv),
            |req: &'static str, srv| srv.call(()).map(move |res| (req, res)),
        );
        if let Poll::Ready(Ok(mut srv)) = new_srv.new_service(&()).try_poll() {
            assert!(srv.poll_ready().is_ready());
            let res = srv.call("srv").try_poll();
            assert!(res.is_ready());
            assert_eq!(res, Poll::Ready(Ok(("srv", ()))));
        } else {
            panic!()
        }
    }
}
