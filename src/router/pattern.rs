//! Route patterns.
//!
//! | segment          | matches                                   |
//! |------------------|-------------------------------------------|
//! | `about`          | exactly `about`                           |
//! | `{id}`           | any segment                               |
//! | `{id=int}`       | a segment the `int` matcher accepts       |
//! | `{?lang}`        | a segment, or nothing                     |
//! | `{?lang=locale}` | a segment the matcher accepts, or nothing |
//! | `{*rest}`        | zero or more segments (last only)         |

use std::collections::HashMap;
use std::rc::Rc;

use percent_encoding::percent_decode_str;

use crate::request::Param;

/// Accepts or rejects a path segment, see [`Router::matcher`](crate::Router::matcher).
pub(crate) type Matcher = Rc<dyn Fn(&str) -> bool>;
pub(crate) type Matchers = HashMap<String, Matcher>;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    Static(String),
    Param {
        name: String,
        matcher: Option<String>,
    },
    Optional {
        name: String,
        matcher: Option<String>,
    },
    Wildcard(String),
}

/// How specific a matched segment is; higher wins. A route's specificity is the sequence of its
/// segments' ranks, compared lexicographically, so `/blog/new` beats `/blog/{slug}`.
mod rank {
    pub(super) const STATIC: u8 = 6;
    pub(super) const MATCHED_PARAM: u8 = 5;
    pub(super) const PARAM: u8 = 4;
    pub(super) const MATCHED_OPTIONAL: u8 = 3;
    pub(super) const OPTIONAL: u8 = 2;
    pub(super) const SKIPPED_OPTIONAL: u8 = 1;
    pub(super) const WILDCARD: u8 = 0;
}

#[derive(Debug, Clone)]
pub(crate) struct Pattern {
    raw: String,
    segments: Vec<Segment>,
}

/// How specific a match is, compared lexicographically.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Specificity(Vec<u8>);

pub(crate) struct Match {
    pub(crate) specificity: Specificity,
    pub(crate) params: Vec<Param>,
}

impl Pattern {
    /// Parses a pattern. Panics on invalid patterns.
    pub(crate) fn parse(raw: &str) -> Self {
        let raw = normalize(raw);
        let segments: Vec<Segment> = split(&raw)
            .map(|segment| parse_segment(&raw, segment))
            .collect();

        if let Some(pos) = segments
            .iter()
            .position(|s| matches!(s, Segment::Wildcard(_)))
        {
            assert!(
                pos == segments.len() - 1,
                "invalid route `{raw}`: a wildcard must be the last segment"
            );
        }
        let mut names: Vec<&str> = segments.iter().filter_map(Segment::param_name).collect();
        names.sort_unstable();
        assert!(
            names.windows(2).all(|w| w[0] != w[1]),
            "invalid route `{raw}`: duplicate parameter name"
        );

        Self { raw, segments }
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.raw
    }

    /// The number of parameters, i.e. the length of `Match::params`.
    pub(crate) fn param_count(&self) -> usize {
        self.segments.iter().filter_map(Segment::param_name).count()
    }

    pub(crate) fn matcher_names(&self) -> impl Iterator<Item = &str> {
        self.segments.iter().filter_map(|segment| match segment {
            Segment::Param { matcher, .. } | Segment::Optional { matcher, .. } => {
                matcher.as_deref()
            }
            _ => None,
        })
    }

    /// Two patterns that match exactly the same paths, equally specific.
    pub(crate) fn conflicts_with(&self, other: &Self) -> bool {
        self.segments.len() == other.segments.len()
            && self
                .segments
                .iter()
                .zip(&other.segments)
                .all(|(a, b)| match (a, b) {
                    (Segment::Static(a), Segment::Static(b)) => a == b,
                    (Segment::Param { matcher: a, .. }, Segment::Param { matcher: b, .. })
                    | (
                        Segment::Optional { matcher: a, .. },
                        Segment::Optional { matcher: b, .. },
                    ) => a == b,
                    (Segment::Wildcard(_), Segment::Wildcard(_)) => true,
                    _ => false,
                })
    }

    pub(crate) fn matches(&self, path: &str, matchers: &Matchers) -> Option<Match> {
        let parts: Vec<String> = split(path).map(decode).collect();
        let mut ranks = Vec::with_capacity(self.segments.len());
        let mut params = Vec::with_capacity(self.param_count());
        let matched = match_from(&self.segments, &parts, matchers, &mut ranks, &mut params);
        matched.then_some(Match {
            specificity: Specificity(ranks),
            params,
        })
    }
}

impl Segment {
    fn param_name(&self) -> Option<&str> {
        match self {
            Self::Param { name, .. } | Self::Optional { name, .. } | Self::Wildcard(name) => {
                Some(name)
            }
            Self::Static(_) => None,
        }
    }
}

fn parse_segment(raw: &str, segment: &str) -> Segment {
    let Some(inner) = segment.strip_prefix('{').and_then(|s| s.strip_suffix('}')) else {
        assert!(
            !segment.contains(['{', '}']),
            "invalid route `{raw}`: parameters must span a whole segment"
        );
        return Segment::Static(segment.to_owned());
    };
    if let Some(name) = inner.strip_prefix('*') {
        return Segment::Wildcard(valid_name(raw, name));
    }
    let (optional, inner) = match inner.strip_prefix('?') {
        Some(inner) => (true, inner),
        None => (false, inner),
    };
    let (name, matcher) = match inner.split_once('=') {
        Some((name, matcher)) => (name, Some(valid_name(raw, matcher))),
        None => (inner, None),
    };
    let name = valid_name(raw, name);
    if optional {
        Segment::Optional { name, matcher }
    } else {
        Segment::Param { name, matcher }
    }
}

/// Matches `segments` against `parts`, backtracking over optional segments (which prefer
/// consuming a part over being skipped).
fn match_from(
    segments: &[Segment],
    parts: &[String],
    matchers: &Matchers,
    ranks: &mut Vec<u8>,
    params: &mut Vec<Param>,
) -> bool {
    let Some((segment, rest)) = segments.split_first() else {
        return parts.is_empty();
    };
    let (ranks_len, params_len) = (ranks.len(), params.len());
    let accepts = |matcher: &Option<String>, part: &str| {
        matcher.as_ref().is_none_or(|name| matchers[name](part))
    };
    let mut attempt = |rank: u8, param: Option<Param>, consumed: usize| {
        ranks.push(rank);
        params.extend(param);
        if match_from(rest, &parts[consumed..], matchers, ranks, params) {
            return true;
        }
        ranks.truncate(ranks_len);
        params.truncate(params_len);
        false
    };

    match segment {
        Segment::Static(s) => parts.first() == Some(s) && attempt(rank::STATIC, None, 1),
        Segment::Param { name, matcher } => parts.first().is_some_and(|part| {
            accepts(matcher, part) && {
                let rank = if matcher.is_some() {
                    rank::MATCHED_PARAM
                } else {
                    rank::PARAM
                };
                attempt(rank, Some(param(name, Some(part))), 1)
            }
        }),
        Segment::Optional { name, matcher } => {
            let consumed = parts.first().is_some_and(|part| {
                accepts(matcher, part) && {
                    let rank = if matcher.is_some() {
                        rank::MATCHED_OPTIONAL
                    } else {
                        rank::OPTIONAL
                    };
                    attempt(rank, Some(param(name, Some(part))), 1)
                }
            });
            consumed || attempt(rank::SKIPPED_OPTIONAL, Some(param(name, None)), 0)
        }
        Segment::Wildcard(name) => {
            let rest = parts.join("/");
            attempt(rank::WILDCARD, Some(param(name, Some(&rest))), parts.len())
        }
    }
}

fn param(name: &str, value: Option<&str>) -> Param {
    Param {
        name: name.to_owned(),
        value: value.map(str::to_owned),
    }
}

fn valid_name(raw: &str, name: &str) -> String {
    assert!(
        !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_'),
        "invalid route `{raw}`: bad parameter or matcher name `{name}`"
    );
    name.to_owned()
}

fn split(path: &str) -> impl Iterator<Item = &str> {
    path.split('/').filter(|s| !s.is_empty())
}

fn decode(segment: &str) -> String {
    percent_decode_str(segment).decode_utf8_lossy().into_owned()
}

/// `blog//post/` -> `/blog/post`
pub(crate) fn normalize(path: &str) -> String {
    let joined = split(path).collect::<Vec<_>>().join("/");
    format!("/{joined}")
}

/// Joins a nest prefix with a route path.
pub(crate) fn join(prefix: &str, path: &str) -> String {
    normalize(&format!("{prefix}/{path}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matchers() -> Matchers {
        let mut matchers = Matchers::new();
        matchers.insert("int".into(), Rc::new(|s: &str| s.parse::<u32>().is_ok()));
        matchers.insert("lang".into(), Rc::new(|s: &str| ["en", "de"].contains(&s)));
        matchers
    }

    fn params(m: &Match) -> Vec<(&str, Option<&str>)> {
        m.params
            .iter()
            .map(|p| (p.name.as_str(), p.value.as_deref()))
            .collect()
    }

    #[test]
    fn matching() {
        let p = Pattern::parse("/users/{id}/posts/{post}");
        let m = p
            .matches("/users/3/posts/hello%20world", &matchers())
            .unwrap();
        assert_eq!(
            params(&m),
            [("id", Some("3")), ("post", Some("hello world"))]
        );
        assert!(p.matches("/users/3/posts", &matchers()).is_none());
        assert!(p.matches("/users/3/posts/a/b", &matchers()).is_none());
    }

    #[test]
    fn wildcard_matches_zero_or_more() {
        let p = Pattern::parse("/files/{*rest}");
        assert_eq!(
            params(&p.matches("/files", &matchers()).unwrap()),
            [("rest", Some(""))]
        );
        assert_eq!(
            params(&p.matches("/files/a/b", &matchers()).unwrap()),
            [("rest", Some("a/b"))]
        );
    }

    #[test]
    fn matchers_filter_and_rank() {
        let m = matchers();
        let by_id = Pattern::parse("/authors/{id=int}");
        #[expect(
            clippy::literal_string_with_formatting_args,
            reason = "route patterns use `{param}` syntax"
        )]
        let by_name = Pattern::parse("/authors/{name}");
        assert!(by_id.matches("/authors/ferris", &m).is_none());
        let (id, name) = (
            by_id.matches("/authors/1", &m).unwrap(),
            by_name.matches("/authors/1", &m).unwrap(),
        );
        assert!(id.specificity > name.specificity);
        assert!(!by_id.conflicts_with(&by_name));
    }

    #[test]
    fn optional_params() {
        let m = matchers();
        let p = Pattern::parse("/{?lang=lang}/about");
        assert_eq!(
            params(&p.matches("/de/about", &m).unwrap()),
            [("lang", Some("de"))]
        );
        assert_eq!(params(&p.matches("/about", &m).unwrap()), [("lang", None)]);
        assert!(p.matches("/fr/about", &m).is_none());

        // Backtracking: the optional gives up its segment when the rest needs it.
        let p = Pattern::parse("/{?a}/{b}");
        assert_eq!(
            params(&p.matches("/x", &m).unwrap()),
            [("a", None), ("b", Some("x"))]
        );

        let fixed = Pattern::parse("/about").matches("/about", &m).unwrap();
        let optional = Pattern::parse("/{?lang}/about")
            .matches("/about", &m)
            .unwrap();
        assert!(fixed.specificity > optional.specificity);
    }

    #[test]
    fn specificity() {
        let m = matchers();
        let fixed = Pattern::parse("/blog/new")
            .matches("/blog/new", &m)
            .unwrap();
        let param = Pattern::parse("/blog/{slug}")
            .matches("/blog/new", &m)
            .unwrap();
        let wild = Pattern::parse("/{*rest}").matches("/blog/new", &m).unwrap();
        assert!(fixed.specificity > param.specificity && param.specificity > wild.specificity);
    }

    #[test]
    fn normalizing() {
        assert_eq!(normalize(""), "/");
        assert_eq!(normalize("blog//a/"), "/blog/a");
        assert_eq!(join("/", "/"), "/");
        assert_eq!(join("/blog", "/"), "/blog");
        assert_eq!(join("/blog/", "{slug}"), "/blog/{slug}");
    }

    #[test]
    #[should_panic(expected = "last segment")]
    fn wildcard_must_be_last() {
        Pattern::parse("/{*rest}/x");
    }
}
