//! [`Request`]: the url being navigated to and the parameters of the matched route, which also
//! records what each load reads from it.

use core::cell::Cell;
use std::fmt;
use std::rc::Rc;

use crate::extract::parent::ParentHandle;

/// A path parameter. Optional parameters (`{?lang}`) that are absent have no value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Param {
    pub(crate) name: String,
    pub(crate) value: Option<String>,
}

/// What a `load` read from its [`Request`], to decide whether it has to rerun on the next
/// navigation. Recorded automatically.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Deps {
    pub(crate) path: bool,
    pub(crate) query: bool,
    pub(crate) params: bool,
    pub(crate) parent: bool,
}

/// The url being navigated to, together with the path parameters of the matched route.
///
/// Hooks and layout loads only see the parameters of their own router's prefix; pages see all.
#[derive(Clone)]
pub struct Request {
    path: String,
    query: Option<String>,
    params: Vec<Param>,
    route: String,
    pub(crate) deps: Rc<Cell<Deps>>,
    pub(crate) parent: Option<ParentHandle>,
}

/// Splits `url` into the part before the `#` and the fragment after it (`None` if empty).
pub(crate) fn split_fragment(url: &str) -> (&str, Option<&str>) {
    match url.split_once('#') {
        Some((url, fragment)) => (url, Some(fragment).filter(|f| !f.is_empty())),
        None => (url, None),
    }
}

impl Request {
    /// The request for `url`. A `#fragment` is dropped: it never affects matching or loads.
    pub(crate) fn new(url: &str) -> Self {
        let (url, _) = split_fragment(url);
        let (path, query) = match url.split_once('?') {
            Some((path, query)) => (path, Some(query.to_owned())),
            None => (url, None),
        };
        Self {
            path: crate::router::normalize(path),
            query: query.filter(|q| !q.is_empty()),
            params: Vec::new(),
            route: String::new(),
            deps: Rc::default(),
            parent: None,
        }
    }

    pub(crate) fn with_route(mut self, route: &str, params: Vec<Param>) -> Self {
        self.route = route.to_owned();
        self.params = params;
        self
    }

    /// A copy for one hook or load: only the first `params` parameters, a fresh dependency
    /// tracker, and access to the parent layouts' data.
    pub(crate) fn for_handler(&self, params: usize, parent: Option<ParentHandle>) -> Self {
        Self {
            params: self.params[..params.min(self.params.len())].to_vec(),
            deps: Rc::default(),
            parent,
            ..self.clone()
        }
    }

    fn track(&self, update: impl FnOnce(&mut Deps)) {
        let mut deps = self.deps.get();
        update(&mut deps);
        self.deps.set(deps);
    }

    /// The path, e.g. `/blog/hello-world`.
    pub fn path(&self) -> &str {
        self.track(|deps| deps.path = true);
        &self.path
    }

    /// The raw query string without the leading `?`.
    pub fn query(&self) -> Option<&str> {
        self.track(|deps| deps.query = true);
        self.query.as_deref()
    }

    /// Path and query, e.g. `/blog?page=2`.
    pub fn url(&self) -> String {
        self.track(|deps| {
            deps.path = true;
            deps.query = true;
        });
        self.untracked_url()
    }

    /// The pattern of the matched route, e.g. `/blog/{slug}`.
    pub fn route(&self) -> &str {
        &self.route
    }

    /// A single (percent-decoded) path parameter. `None` if the route has no such parameter,
    /// or it is an optional parameter that is absent.
    pub fn param(&self, name: &str) -> Option<&str> {
        self.track(|deps| deps.params = true);
        self.params
            .iter()
            .find(|param| param.name == name)
            .and_then(|param| param.value.as_deref())
    }

    pub(crate) fn params(&self) -> &[Param] {
        self.track(|deps| deps.params = true);
        &self.params
    }

    pub(crate) fn untracked_path(&self) -> &str {
        &self.path
    }

    pub(crate) fn untracked_url(&self) -> String {
        match &self.query {
            Some(query) => format!("{}?{query}", self.path),
            None => self.path.clone(),
        }
    }

    /// Whether a load that depended on `deps` of `self` sees the same values in `other`.
    pub(crate) fn same_for(&self, deps: Deps, other: &Self) -> bool {
        (!deps.path || self.path == other.path)
            && (!deps.query || self.query == other.query)
            && (!deps.params || self.params == other.params)
    }
}

impl fmt::Debug for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Request")
            .field("path", &self.path)
            .field("query", &self.query)
            .field("params", &self.params)
            .field("route", &self.route)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragments_are_dropped() {
        assert_eq!(split_fragment("/a?b#c"), ("/a?b", Some("c")));
        assert_eq!(split_fragment("/a#"), ("/a", None));
        assert_eq!(split_fragment("/a"), ("/a", None));

        let request = Request::new("/docs/#install");
        assert_eq!(request.untracked_url(), "/docs");
        let request = Request::new("/blog?tag=x#top");
        assert_eq!(request.untracked_path(), "/blog");
        assert_eq!(request.query(), Some("tag=x"));
        assert_eq!(Request::new("/blog?#top").query(), None);
    }
}
