//! [`href`]: building urls from route patterns.

use core::fmt;

use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use serde::Serialize;

use crate::router::Pattern;

/// What a fragment can't contain unencoded, as in the URL standard.
const FRAGMENT: &AsciiSet = &CONTROLS.add(b' ').add(b'"').add(b'<').add(b'>').add(b'`');

/// Builds a url from a route pattern, percent-encoding the parameters, so that the router
/// matches it and decodes the same values.
///
/// ```
/// use egui_kit::href;
/// use serde::Serialize;
///
/// #[derive(Serialize)]
/// struct BlogQuery {
///     tag: Option<String>,
///     page: u32,
/// }
///
/// let url = href("/blog/{slug}").param("slug", "hello world");
/// assert_eq!(url.to_string(), "/blog/hello%20world");
///
/// let query = BlogQuery { tag: Some("egui".into()), page: 2 };
/// let url = href("/{?lang}/blog").query(&query).fragment("top");
/// assert_eq!(url.to_string(), "/blog?tag=egui&page=2#top");
/// ```
///
/// [`Nav::goto`](crate::extract::Nav::goto) and friends take it directly,
/// [`Nav::link`](crate::extract::Nav::link) as `&href(..).to_string()`.
///
/// Panics on an invalid pattern, like [`Router::page`](crate::Router::page). Turning it into a
/// url panics if a required parameter is missing.
pub fn href(pattern: &str) -> Href {
    Href {
        pattern: Pattern::parse(pattern),
        params: Vec::new(),
        query: None,
        fragment: None,
    }
}

/// A url being built from a route pattern, see [`href`]. Turned into a url with
/// [`Display`](fmt::Display) or [`String::from`].
#[derive(Debug, Clone)]
#[must_use]
pub struct Href {
    pattern: Pattern,
    params: Vec<(String, String)>,
    query: Option<String>,
    fragment: Option<String>,
}

impl Href {
    /// Sets the parameter `name`: `{name}`, `{name=matcher}`, `{?name}` (left out of the url
    /// unless set) or `{*name}` (whose value may contain slashes).
    ///
    /// Panics if the pattern has no such parameter.
    pub fn param(mut self, name: &str, value: impl fmt::Display) -> Self {
        assert!(
            self.pattern.has_param(name),
            "`{}` has no parameter `{name}`",
            self.pattern.as_str()
        );
        let value = value.to_string();
        match self.params.iter_mut().find(|(param, _)| param == name) {
            Some((_, old)) => *old = value,
            None => self.params.push((name.to_owned(), value)),
        }
        self
    }

    /// Sets the query from a flat struct or a sequence of pairs, with `serde_urlencoded` (the
    /// format [`Query`](crate::extract::Query) reads). `None` fields are left out, and an empty
    /// query adds no `?`. Replaces an earlier query.
    ///
    /// Panics if `query` can't be serialized, e.g. a struct with nested structs.
    pub fn query(mut self, query: &impl Serialize) -> Self {
        let query = serde_urlencoded::to_string(query)
            .unwrap_or_else(|error| panic!("href: can't serialize the query: {error}"));
        self.query = Some(query).filter(|query| !query.is_empty());
        self
    }

    /// Sets the `#fragment` (without the `#`). Spaces, quotes, `<`, `>` and backticks are
    /// percent-encoded.
    pub fn fragment(mut self, fragment: &str) -> Self {
        self.fragment = Some(utf8_percent_encode(fragment, FRAGMENT).to_string());
        self
    }
}

impl fmt::Display for Href {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.pattern.fill(&self.params))?;
        if let Some(query) = &self.query {
            write!(f, "?{query}")?;
        }
        if let Some(fragment) = &self.fragment {
            write!(f, "#{fragment}")?;
        }
        Ok(())
    }
}

impl From<Href> for String {
    fn from(href: Href) -> Self {
        href.to_string()
    }
}

#[cfg(test)]
mod tests {
    use serde::Serialize;

    use super::href;

    #[derive(Serialize)]
    struct Filter {
        tag: Option<&'static str>,
        q: &'static str,
    }

    #[test]
    fn params() {
        let url = href("/users/{id=int}/{?tab}").param("id", 7);
        assert_eq!(url.to_string(), "/users/7");
        let url = url.param("tab", "posts").param("id", 8);
        assert_eq!(String::from(url), "/users/8/posts");
        let url = href("/files/{*path}").param("path", "a b/c?");
        assert_eq!(url.to_string(), "/files/a%20b/c%3F");
    }

    #[test]
    fn query_and_fragment() {
        let filter = Filter {
            tag: None,
            q: "a&b c",
        };
        let url = href("/blog").query(&filter).fragment("top");
        assert_eq!(url.to_string(), "/blog?q=a%26b+c#top");
        let url = href("/blog").query(&[("tag", "x")]).query(&());
        assert_eq!(url.to_string(), "/blog");
        assert_eq!(href("/").fragment("a b").to_string(), "/#a%20b");
    }

    #[test]
    #[should_panic(expected = "`/blog/{slug}` is missing the parameter `slug`")]
    fn missing_param() {
        let _ = href("/blog/{slug}").to_string();
    }

    #[test]
    #[should_panic(expected = "`/blog/{slug}` has no parameter `id`")]
    fn unknown_param() {
        let _ = href("/blog/{slug}").param("id", 1);
    }

    #[test]
    #[should_panic(expected = "href: can't serialize the query")]
    fn nested_query() {
        #[derive(Serialize)]
        struct Outer {
            inner: Filter,
        }
        let outer = Outer {
            inner: Filter { tag: None, q: "" },
        };
        let _ = href("/").query(&outer);
    }

    #[test]
    #[should_panic(expected = "invalid route")]
    fn invalid_pattern() {
        let _ = href("/{*rest}/x");
    }
}
