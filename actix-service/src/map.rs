use std::marker::PhantomData;

use futures::{TryFuture, Poll};

use super::{NewService, Service};
use std::pin::Pin;
use futures::task::Context;

/// Service for the `map` combinator, changing the type of a service's response.
///
/// This is created by the `ServiceExt::map` method.
pub struct Map<A, F, Response> {
    service: A,
    f: F,
    _t: PhantomData<Response>,
}

impl<A, F, Response> Map<A, F, Response> {
    /// Create new `Map` combinator
    pub fn new(service: A, f: F) -> Self
    where
        A: Service,
        F: FnMut(A::Response) -> Response,
    {
        Self {
            service,
            f,
            _t: PhantomData,
        }
    }
}

impl<A, F, Response> Clone for Map<A, F, Response>
where
    A: Clone,
    F: Clone,
{
    fn clone(&self) -> Self {
        Map {
            service: self.service.clone(),
            f: self.f.clone(),
            _t: PhantomData,
        }
    }
}

impl<A, F, Response> Service for Map<A, F, Response>
where
    A: Service,
    F: FnMut(A::Response) -> Response + Clone,
{
    type Request = A::Request;
    type Response = Response;
    type Error = A::Error;
    type Future = MapFuture<A, F, Response>;

    fn poll_ready(&mut self) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready()
    }

    fn call(&mut self, req: A::Request) -> Self::Future {
        MapFuture::new(self.service.call(req), self.f.clone())
    }
}

pub struct MapFuture<A, F, Response>
where
    A: Service,
    F: FnMut(A::Response) -> Response,
{
    f: F,
    fut: A::Future,
}

impl<A, F, Response> MapFuture<A, F, Response>
where
    A: Service,
    F: FnMut(A::Response) -> Response,
{
    fn new(fut: A::Future, f: F) -> Self {
        MapFuture { f, fut }
    }
}

impl<A, F, Response> TryFuture for MapFuture<A, F, Response>
where
    A: Service,
    F: FnMut(A::Response) -> Response,
{
    type Ok = Response;
    type Error = A::Error;

    fn try_poll(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        match self.fut.try_poll() {
            Poll::Ready(resp) => Poll::Ready(Ok((self.f)(resp?))),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// `MapNewService` new service combinator
pub struct MapNewService<A, F, Res> {
    a: A,
    f: F,
    r: PhantomData<Res>,
}

impl<A, F, Res> MapNewService<A, F, Res> {
    /// Create new `Map` new service instance
    pub fn new(a: A, f: F) -> Self
    where
        A: NewService,
        F: FnMut(A::Response) -> Res,
    {
        Self {
            a,
            f,
            r: PhantomData,
        }
    }
}

impl<A, F, Res> Clone for MapNewService<A, F, Res>
where
    A: Clone,
    F: Clone,
{
    fn clone(&self) -> Self {
        Self {
            a: self.a.clone(),
            f: self.f.clone(),
            r: PhantomData,
        }
    }
}

impl<A, F, Res> NewService for MapNewService<A, F, Res>
where
    A: NewService,
    F: FnMut(A::Response) -> Res + Clone,
{
    type Request = A::Request;
    type Response = Res;
    type Error = A::Error;

    type Config = A::Config;
    type Service = Map<A::Service, F, Res>;
    type InitError = A::InitError;
    type Future = MapNewServiceFuture<A, F, Res>;

    fn new_service(&self, cfg: &A::Config) -> Self::Future {
        MapNewServiceFuture::new(self.a.new_service(cfg), self.f.clone())
    }
}

pub struct MapNewServiceFuture<A, F, Res>
where
    A: NewService,
    F: FnMut(A::Response) -> Res,
{
    fut: A::Future,
    f: Option<F>,
}

impl<A, F, Res> MapNewServiceFuture<A, F, Res>
where
    A: NewService,
    F: FnMut(A::Response) -> Res,
{
    fn new(fut: A::Future, f: F) -> Self {
        MapNewServiceFuture { f: Some(f), fut }
    }
}

impl<A, F, Res> TryFuture for MapNewServiceFuture<A, F, Res>
where
    A: NewService,
    F: FnMut(A::Response) -> Res,
{
    type Ok = Map<A::Service, F, Res>;
    type Error = A::InitError;

    fn try_poll(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        if let Poll::Ready(service) = self.fut.try_poll() {
            Ok(Poll::Ready(Map::new(service?, self.f.take().unwrap())))
        } else {
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod tests {
    use futures::future::{ok, Ready};

    use super::*;
    use crate::{IntoNewService, Service, ServiceExt};

    struct Srv;
    impl Service for Srv {
        type Request = ();
        type Response = ();
        type Error = ();
        type Future = Ready<Result<(), ()>>;

        fn poll_ready(&mut self) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, _: ()) -> Self::Future {
            ok(())
        }
    }

    #[test]
    fn test_poll_ready() {
        let mut srv = Srv.map(|_| "ok");
        let res = srv.poll_ready();
        assert!(res.is_ready());
        assert_eq!(res, Poll::Ready(Ok(())));
    }

    #[test]
    fn test_call() {
        let mut srv = Srv.map(|_| "ok");
        let res = srv.call(()).try_poll();
        assert!(res.is_ready());
        assert_eq!(res, Poll::Ready(Ok("ok")));
    }

    #[test]
    fn test_new_service() {
        let blank = || Ok::<_, ()>(Srv);
        let new_srv = blank.into_new_service().map(|_| "ok");
        if let Poll::Ready(Ok(mut srv)) = new_srv.new_service(&()).try_poll() {
            let res = srv.call(()).try_poll();
            assert!(res.is_ready());
            assert_eq!(res, Poll::Ready(Ok("ok")));
        } else {
            panic!()
        }
    }
}
