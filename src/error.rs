//! What navigations fail with: [`Rejection`], made of a [`RouteError`] or a [`Redirect`].

use core::convert::Infallible;
use std::fmt;

/// An error that aborts a navigation and renders the nearest error page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteError {
    pub status: u16,
    pub message: String,
}

impl RouteError {
    pub fn new(status: u16, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(404, message)
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(400, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(500, message)
    }
}

impl fmt::Display for RouteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.status, self.message)
    }
}

impl core::error::Error for RouteError {}

/// Aborts a navigation and navigates to another url instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redirect {
    pub to: String,
}

impl Redirect {
    pub fn to(to: impl Into<String>) -> Self {
        Self { to: to.into() }
    }
}

/// Everything a hook, a `load` handler or an extractor can abort a navigation with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejection {
    Error(RouteError),
    Redirect(Redirect),
}

impl From<RouteError> for Rejection {
    fn from(err: RouteError) -> Self {
        Self::Error(err)
    }
}

impl From<Redirect> for Rejection {
    fn from(redirect: Redirect) -> Self {
        Self::Redirect(redirect)
    }
}

impl From<Infallible> for Rejection {
    fn from(never: Infallible) -> Self {
        match never {}
    }
}

/// Shorthand for `Rejection::Error(RouteError::new(status, message))`.
pub fn error(status: u16, message: impl Into<String>) -> Rejection {
    RouteError::new(status, message).into()
}

/// Shorthand for `Rejection::Redirect(Redirect::to(to))`.
pub fn redirect(to: impl Into<String>) -> Rejection {
    Redirect::to(to).into()
}
