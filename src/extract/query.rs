//! [`Query`]: typed query parameters.

use serde::de::DeserializeOwned;

use super::FromRequest;
use crate::error::{Rejection, RouteError};
use crate::request::Request;

/// Typed query parameters, deserialized with `serde_urlencoded`. Fails with a 400.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Query<T>(pub T);

impl<S, T: DeserializeOwned> FromRequest<S> for Query<T> {
    fn from_request(req: &Request, _state: &S) -> Result<Self, Rejection> {
        serde_urlencoded::from_str(req.query().unwrap_or(""))
            .map(Query)
            .map_err(|err| RouteError::bad_request(format!("invalid query: {err}")).into())
    }
}
