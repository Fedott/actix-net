use std::marker::PhantomData;

use futures::{TryFuture, Poll};

use super::{NewService, Service};
use std::pin::Pin;
use futures::task::Context;

/// Service for the `from_err` combinator, changing the error type of a service.
///
/// This is created by the `ServiceExt::from_err` method.
pub struct FromErr<A, E> {
    service: A,
    f: PhantomData<E>,
}

impl<A, E> FromErr<A, E> {
    pub(crate) fn new(service: A) -> Self
    where
        A: Service,
        E: From<A::Error>,
    {
        FromErr {
            service,
            f: PhantomData,
        }
    }
}

impl<A, E> Clone for FromErr<A, E>
where
    A: Clone,
{
    fn clone(&self) -> Self {
        FromErr {
            service: self.service.clone(),
            f: PhantomData,
        }
    }
}

impl<A, E> Service for FromErr<A, E>
where
    A: Service,
    E: From<A::Error>,
{
    type Request = A::Request;
    type Response = A::Response;
    type Error = E;
    type Future = FromErrFuture<A, E>;

    fn poll_ready(&mut self) -> Poll<Result<(), E>> {
        self.service.poll_ready().map_err(E::from)
    }

    fn call(&mut self, req: A::Request) -> Self::Future {
        FromErrFuture {
            fut: self.service.call(req),
            f: PhantomData,
        }
    }
}

pub struct FromErrFuture<A: Service, E> {
    fut: A::Future,
    f: PhantomData<E>,
}

impl<A, E> TryFuture for FromErrFuture<A, E>
where
    A: Service,
    E: From<A::Error>,
{
    type Ok = A::Response;
    type Error = E;

    fn try_poll(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        self.fut.try_poll().map_err(E::from)
    }
}

/// NewService for the `from_err` combinator, changing the type of a new
/// service's error.
///
/// This is created by the `NewServiceExt::from_err` method.
pub struct FromErrNewService<A, E> {
    a: A,
    e: PhantomData<E>,
}

impl<A, E> FromErrNewService<A, E> {
    /// Create new `FromErr` new service instance
    pub fn new(a: A) -> Self
    where
        A: NewService,
        E: From<A::Error>,
    {
        Self { a, e: PhantomData }
    }
}

impl<A, E> Clone for FromErrNewService<A, E>
where
    A: Clone,
{
    fn clone(&self) -> Self {
        Self {
            a: self.a.clone(),
            e: PhantomData,
        }
    }
}

impl<A, E> NewService for FromErrNewService<A, E>
where
    A: NewService,
    E: From<A::Error>,
{
    type Request = A::Request;
    type Response = A::Response;
    type Error = E;

    type Config = A::Config;
    type Service = FromErr<A::Service, E>;
    type InitError = A::InitError;
    type Future = FromErrNewServiceFuture<A, E>;

    fn new_service(&self, cfg: &A::Config) -> Self::Future {
        FromErrNewServiceFuture {
            fut: self.a.new_service(cfg),
            e: PhantomData,
        }
    }
}

pub struct FromErrNewServiceFuture<A, E>
where
    A: NewService,
    E: From<A::Error>,
{
    fut: A::Future,
    e: PhantomData<E>,
}

impl<A, E> TryFuture for FromErrNewServiceFuture<A, E>
where
    A: NewService,
    E: From<A::Error>,
{
    type Ok = FromErr<A::Service, E>;
    type Error = A::InitError;

    fn try_poll(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        if let Poll::Ready(service) = self.fut.try_poll() {
            Poll::Ready(Ok(FromErr::new(service?)))
        } else {
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod tests {
    use futures::future::{err, Ready};

    use super::*;
    use crate::{IntoNewService, NewService, Service, ServiceExt};

    struct Srv;
    impl Service for Srv {
        type Request = ();
        type Response = ();
        type Error = ();
        type Future = Ready<Result<(), ()>>;

        fn poll_ready(&mut self) -> Poll<Result<(), Self::Error>> {
            Err(())
        }

        fn call(&mut self, _: ()) -> Self::Future {
            err(())
        }
    }

    #[derive(Debug, PartialEq)]
    struct Error;

    impl From<()> for Error {
        fn from(_: ()) -> Self {
            Error
        }
    }

    #[test]
    fn test_poll_ready() {
        let mut srv = Srv.from_err::<Error>();
        let res = srv.poll_ready();
        assert!(res.is_ready());
        assert_eq!(res, Poll::Ready(Err(Error)));
    }

    #[test]
    fn test_call() {
        let mut srv = Srv.from_err::<Error>();
        let res = srv.call(()).try_poll();
        assert!(res.is_ready());
        assert_eq!(res, Poll::Ready(Err(Error)));
    }

    #[test]
    fn test_new_service() {
        let blank = || Ok::<_, ()>(Srv);
        let new_srv = blank.into_new_service().from_err::<Error>();
        if let Poll::Ready(Ok(mut srv)) = new_srv.new_service(&()).try_poll() {
            let res = srv.call(()).try_poll();
            assert!(res.is_ready());
            assert_eq!(res, Poll::Ready(Err(Error)));
        } else {
            panic!()
        }
    }
}
