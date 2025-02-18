use crate::Builder;

use tokio_executor::current_thread::Handle as ExecutorHandle;
use tokio_executor::current_thread::{self, CurrentThread};
use tokio_net::driver::{self, Reactor};
use tokio_timer::clock::{self, Clock};
use tokio_timer::timer::{self, Timer};

use std::error::Error;
use std::fmt;
use std::future::Future;
use std::io;
use tokio_executor as executor;

/// Single-threaded runtime provides a way to start reactor
/// and executor on the current thread.
///
/// See [module level][mod] documentation for more details.
///
/// [mod]: index.html
#[derive(Debug)]
pub struct Runtime {
    reactor_handle: driver::Handle,
    timer_handle: timer::Handle,
    clock: Clock,
    executor: CurrentThread<Parker>,
}

pub(super) type Parker = Timer<Reactor>;

/// Handle to spawn a future on the corresponding `CurrentThread` runtime instance
#[derive(Debug, Clone)]
pub struct Handle(ExecutorHandle);

impl Handle {
    /// Spawn a future onto the `CurrentThread` runtime instance corresponding to this handle
    ///
    /// # Panics
    ///
    /// This function panics if the spawn fails. Failure occurs if the `CurrentThread`
    /// instance of the `Handle` does not exist anymore.
    pub fn spawn<F>(&self, future: F) -> Result<(), tokio_executor::SpawnError>
        where
            F: Future<Output = ()> + Send + 'static,
    {
        self.0.spawn(future)
    }

    /// Provides a best effort **hint** to whether or not `spawn` will succeed.
    ///
    /// This function may return both false positives **and** false negatives.
    /// If `status` returns `Ok`, then a call to `spawn` will *probably*
    /// succeed, but may fail. If `status` returns `Err`, a call to `spawn` will
    /// *probably* fail, but may succeed.
    ///
    /// This allows a caller to avoid creating the task if the call to `spawn`
    /// has a high likelihood of failing.
    pub fn status(&self) -> Result<(), tokio_executor::SpawnError> {
        self.0.status()
    }
}

impl<T> executor::TypedExecutor<T> for Handle
    where
        T: Future<Output = ()> + Send + 'static,
{
    fn spawn(&mut self, future: T) -> Result<(), executor::SpawnError> {
        Handle::spawn(self, future)
    }
}

/// Error returned by the `run` function.
#[derive(Debug)]
pub struct RunError {
    inner: current_thread::RunError,
}

impl fmt::Display for RunError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(fmt, "{}", self.inner)
    }
}

impl Error for RunError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.inner.source()
    }
}

impl Runtime {
    /// Returns a new runtime initialized with default configuration values.
    pub fn new() -> io::Result<Runtime> {
        Builder::new().build_rt()
    }

    pub(super) fn new2(
        reactor_handle: driver::Handle,
        timer_handle: timer::Handle,
        clock: Clock,
        executor: CurrentThread<Parker>,
    ) -> Runtime {
        Runtime {
            reactor_handle,
            timer_handle,
            clock,
            executor,
        }
    }

    /// Get a new handle to spawn futures on the single-threaded Tokio runtime
    ///
    /// Different to the runtime itself, the handle can be sent to different
    /// threads.
    pub fn handle(&self) -> Handle {
        Handle(self.executor.handle().clone())
    }

    /// Spawn a future onto the single-threaded Tokio runtime.
    ///
    /// See [module level][mod] documentation for more details.
    ///
    /// [mod]: index.html
    ///
    /// # Examples
    ///
    /// ```
    /// use tokio::runtime::current_thread::Runtime;
    ///
    /// # fn dox() {
    /// // Create the runtime
    /// let mut rt = Runtime::new().unwrap();
    ///
    /// // Spawn a future onto the runtime
    /// rt.spawn(async {
    ///     println!("now running on a worker thread");
    /// });
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// This function panics if the spawn fails. Failure occurs if the executor
    /// is currently at capacity and is unable to spawn a new future.
    pub fn spawn<F>(&mut self, future: F) -> &mut Self
        where
            F: Future<Output = ()> + 'static,
    {
        self.executor.spawn(future);
        self
    }

    /// Runs the provided future, blocking the current thread until the future
    /// completes.
    ///
    /// This function can be used to synchronously block the current thread
    /// until the provided `future` has resolved either successfully or with an
    /// error. The result of the future is then returned from this function
    /// call.
    ///
    /// Note that this function will **also** execute any spawned futures on the
    /// current thread, but will **not** block until these other spawned futures
    /// have completed. Once the function returns, any uncompleted futures
    /// remain pending in the `Runtime` instance. These futures will not run
    /// until `block_on` or `run` is called again.
    ///
    /// The caller is responsible for ensuring that other spawned futures
    /// complete execution by calling `block_on` or `run`.
    pub fn block_on<F>(&mut self, f: F) -> F::Output
        where
            F: Future,
    {
        self.enter(|executor| {
            // Run the provided future
            executor.block_on(f)
        })
    }

    /// Run the executor to completion, blocking the thread until **all**
    /// spawned futures have completed.
    pub fn run(&mut self) -> Result<(), RunError> {
        self.enter(|executor| executor.run())
            .map_err(|e| RunError { inner: e })
    }

    fn enter<F, R>(&mut self, f: F) -> R
        where
            F: FnOnce(&mut current_thread::CurrentThread<Parker>) -> R,
    {
        let Runtime {
            ref reactor_handle,
            ref timer_handle,
            ref clock,
            ref mut executor,
            ..
        } = *self;

        // This will set the default handle and timer to use inside the closure
        // and run the future.
        let _reactor = driver::set_default(&reactor_handle);
        clock::with_default(clock, || {
            let _timer = timer::set_default(&timer_handle);
            // The TaskExecutor is a fake executor that looks into the
            // current single-threaded executor when used. This is a trick,
            // because we need two mutable references to the executor (one
            // to run the provided future, another to install as the default
            // one). We use the fake one here as the default one.
            let mut default_executor = current_thread::TaskExecutor::current();
            tokio_executor::with_default(&mut default_executor, || f(executor))
        })
    }
}
