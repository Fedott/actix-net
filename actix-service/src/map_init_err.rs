use std::marker::PhantomData;

use futures::{TryFuture, Poll};

use super::NewService;
use std::pin::Pin;
use futures::task::Context;

/// `MapInitErr` service combinator
pub struct MapInitErr<A, F, E> {
    a: A,
    f: F,
    e: PhantomData<E>,
}

impl<A, F, E> MapInitErr<A, F, E> {
    /// Create new `MapInitErr` combinator
    pub fn new(a: A, f: F) -> Self
    where
        A: NewService,
        F: Fn(A::InitError) -> E,
    {
        Self {
            a,
            f,
            e: PhantomData,
        }
    }
}

impl<A, F, E> Clone for MapInitErr<A, F, E>
where
    A: Clone,
    F: Clone,
{
    fn clone(&self) -> Self {
        Self {
            a: self.a.clone(),
            f: self.f.clone(),
            e: PhantomData,
        }
    }
}

impl<A, F, E> NewService for MapInitErr<A, F, E>
where
    A: NewService,
    F: Fn(A::InitError) -> E + Clone,
{
    type Request = A::Request;
    type Response = A::Response;
    type Error = A::Error;

    type Config = A::Config;
    type Service = A::Service;
    type InitError = E;
    type Future = MapInitErrFuture<A, F, E>;

    fn new_service(&self, cfg: &A::Config) -> Self::Future {
        MapInitErrFuture::new(self.a.new_service(cfg), self.f.clone())
    }
}

pub struct MapInitErrFuture<A, F, E>
where
    A: NewService,
    F: Fn(A::InitError) -> E,
{
    f: F,
    fut: A::Future,
}

impl<A, F, E> MapInitErrFuture<A, F, E>
where
    A: NewService,
    F: Fn(A::InitError) -> E,
{
    fn new(fut: A::Future, f: F) -> Self {
        MapInitErrFuture { f, fut }
    }
}

impl<A, F, E> TryFuture for MapInitErrFuture<A, F, E>
where
    A: NewService,
    F: Fn(A::InitError) -> E,
{
    type Ok = A::Service;
    type Error = E;

    fn try_poll(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        self.fut.try_poll().map_err(&self.f)
    }
}
