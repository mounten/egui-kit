//! [`Handler`]: async functions of extractors, used as `load`s and hooks.

use core::any::Any;
use core::future::{Future, ready};
use core::pin::Pin;
use std::rc::Rc;

use crate::error::Rejection;
use crate::extract::FromRequest;
use crate::request::Request;

pub(crate) type LocalBoxFuture<T> = Pin<Box<dyn Future<Output = T>>>;

/// A type-erased handler producing a value (the data of a `load`, or `()` for a hook).
pub(crate) type ErasedHandler<S> =
    Rc<dyn Fn(&Request, &S) -> LocalBoxFuture<Result<Box<dyn Any>, Rejection>>>;

/// An async function whose arguments are all [`FromRequest`] extractors.
///
/// Implemented for `Fn(T1, …, T16) -> impl Future`. `T` is a tuple of
/// the extractor types and only exists to keep the implementations apart.
///
/// The futures do not have to be `Send`, so browser futures (e.g. `fetch`) work on wasm.
/// They are polled once per frame by the router and wake egui up when they make progress.
pub trait Handler<T, S>: 'static {
    type Output;
    type Future: Future<Output = Self::Output> + 'static;

    /// Runs the extractors and creates the handler's future.
    ///
    /// # Errors
    ///
    /// Returns the [`Rejection`] of the first extractor that fails.
    fn call(&self, req: &Request, state: &S) -> Result<Self::Future, Rejection>;
}

macro_rules! impl_handler {
    ($($ty:ident),*) => {
        #[expect(
            clippy::allow_attributes,
            reason = "whether the lints fire depends on the arity, so `expect` would be unfulfilled for some"
        )]
        impl<F, Fut, S, $($ty,)*> Handler<($($ty,)*), S> for F
        where
            F: Fn($($ty),*) -> Fut + 'static,
            Fut: Future + 'static,
            $($ty: FromRequest<S>,)*
        {
            type Output = Fut::Output;
            type Future = Fut;

            #[allow(non_snake_case, unused_variables)]
            fn call(&self, req: &Request, state: &S) -> Result<Fut, Rejection> {
                $(let $ty = $ty::from_request(req, state)?;)*
                Ok(self($($ty),*))
            }
        }
    };
}

crate::all_tuples!(impl_handler);

/// Erases a handler returning `Result<D, E>` into one returning boxed data.
pub(crate) fn erase<H, T, S, D, E>(handler: H) -> ErasedHandler<S>
where
    H: Handler<T, S, Output = Result<D, E>>,
    D: 'static,
    E: Into<Rejection>,
{
    Rc::new(move |req, state| match handler.call(req, state) {
        Ok(fut) => Box::pin(async move {
            fut.await
                .map(|data| Box::new(data) as Box<dyn Any>)
                .map_err(Into::into)
        }),
        Err(rejection) => Box::pin(ready(Err(rejection))),
    })
}

/// A handler that does nothing and yields `()` as data.
pub(crate) fn unit<S: 'static>() -> ErasedHandler<S> {
    Rc::new(|_, _| Box::pin(ready(Ok(Box::new(()) as Box<dyn Any>))))
}
