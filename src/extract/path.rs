//! [`Path`]: typed path parameters.

mod de;

use serde::de::DeserializeOwned;

use super::FromRequest;
use crate::error::{Rejection, RouteError};
use crate::request::Request;
use de::PathDeserializer;

/// Typed path parameters, deserialized with serde.
///
/// `T` can be a single value (`Path<u32>`), a tuple (`Path<(u32, String)>`) or a struct
/// whose field names match the parameter names. Optional parameters (`{?lang}`) need an
/// `Option<_>`. A parameter that fails to parse results in a 404; use a
/// [matcher](crate::Router::matcher) to try other routes instead.
///
/// In a layout's `load`, only the parameters of its router's prefix are available.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Path<T>(pub T);

impl<S, T: DeserializeOwned> FromRequest<S> for Path<T> {
    fn from_request(req: &Request, _state: &S) -> Result<Self, Rejection> {
        T::deserialize(PathDeserializer::new(req.params()))
            .map(Path)
            .map_err(|err| RouteError::not_found(format!("invalid path parameters: {err}")).into())
    }
}
