//! The flattened route tree, and matching urls against it.

use std::rc::Rc;

use super::pattern::{Match, Matchers, Pattern, join};
use crate::page::Page;
use crate::request::Request;
use crate::router::Level;

/// The wildcard of the 404 fallbacks; an implementation detail hidden from extractors.
const FALLBACK_PARAM: &str = "__not_found";

pub(crate) enum Target<S> {
    Page(Rc<Page<S>>),
    NotFound,
}

pub(crate) struct RouteEntry<S> {
    pub(crate) pattern: Pattern,
    /// Outermost first.
    pub(crate) levels: Vec<Rc<Level<S>>>,
    pub(crate) target: Target<S>,
}

pub(crate) struct RouteTable<S> {
    routes: Vec<Rc<RouteEntry<S>>>,
    fallbacks: Vec<Rc<RouteEntry<S>>>,
    pub(crate) matchers: Matchers,
}

impl<S> Default for RouteTable<S> {
    fn default() -> Self {
        Self {
            routes: Vec::new(),
            fallbacks: Vec::new(),
            matchers: Matchers::new(),
        }
    }
}

impl<S> RouteTable<S> {
    pub(crate) fn add_page(&mut self, path: &str, levels: Vec<Rc<Level<S>>>, page: Page<S>) {
        self.routes.push(Rc::new(RouteEntry {
            pattern: Pattern::parse(path),
            levels,
            target: Target::Page(Rc::new(page)),
        }));
    }

    /// Paths below `prefix` that match no route render the 404 of the router at `prefix`.
    pub(crate) fn add_fallback(&mut self, prefix: &str, levels: Vec<Rc<Level<S>>>) {
        self.fallbacks.push(Rc::new(RouteEntry {
            pattern: Pattern::parse(&join(prefix, &format!("{{*{FALLBACK_PARAM}}}"))),
            levels,
            target: Target::NotFound,
        }));
    }

    pub(crate) fn validate(&self) {
        for (i, a) in self.routes.iter().enumerate() {
            for b in &self.routes[i + 1..] {
                assert!(
                    !a.pattern.conflicts_with(&b.pattern),
                    "conflicting routes `{}` and `{}`",
                    a.pattern.as_str(),
                    b.pattern.as_str()
                );
            }
        }
        for entry in self.routes.iter().chain(&self.fallbacks) {
            for name in entry.pattern.matcher_names() {
                assert!(
                    self.matchers.contains_key(name),
                    "route `{}` uses the unknown matcher `{name}`",
                    entry.pattern.as_str()
                );
            }
        }
    }

    /// Finds the most specific route for `url`. Always succeeds, since the root router has a
    /// catch-all fallback.
    pub(crate) fn resolve(&self, url: &str) -> (Rc<RouteEntry<S>>, Request) {
        let request = Request::new(url);
        let path = request.untracked_path();
        let (entry, m) = self
            .best_match(&self.routes, path)
            .or_else(|| self.best_match(&self.fallbacks, path))
            .expect("the root fallback matches every path");

        let mut params = m.params;
        params.retain(|param| param.name != FALLBACK_PARAM);
        let request = request.with_route(entry.pattern.as_str(), params);
        (Rc::clone(entry), request)
    }

    /// The pattern of the page route that `url` matches, `None` for a 404.
    pub(crate) fn page_route(&self, url: &str) -> Option<&str> {
        let request = Request::new(url);
        let (entry, _) = self.best_match(&self.routes, request.untracked_path())?;
        Some(entry.pattern.as_str())
    }

    fn best_match<'t>(
        &self,
        entries: &'t [Rc<RouteEntry<S>>],
        path: &str,
    ) -> Option<(&'t Rc<RouteEntry<S>>, Match)> {
        let mut best: Option<(&Rc<RouteEntry<S>>, Match)> = None;
        for entry in entries {
            if let Some(m) = entry.pattern.matches(path, &self.matchers) {
                // On ties the first registered entry wins, i.e. the outer router.
                if best
                    .as_ref()
                    .is_none_or(|(_, b)| m.specificity > b.specificity)
                {
                    best = Some((entry, m));
                }
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use crate::page::Page;
    use crate::router::Router;

    fn resolve(router: Router<()>, url: &str) -> (String, Vec<(String, Option<String>)>, usize) {
        let table = router.into_table();
        let (entry, req) = table.resolve(url);
        let params = req
            .params()
            .iter()
            .map(|p| (p.name.clone(), p.value.clone()))
            .collect();
        let layouts = entry
            .levels
            .iter()
            .filter(|level| level.layout.is_some())
            .count();
        (req.route().to_owned(), params, layouts)
    }

    fn page() -> Page<()> {
        Page::new(|_: &mut egui::Ui| {})
    }

    fn app() -> Router<()> {
        Router::new()
            .matcher("int", |s| s.parse::<u32>().is_ok())
            .page("/", page())
            .page("/{?lang}/about", page())
            .nest(
                "/users/{id=int}",
                Router::new()
                    .page("/", page())
                    .page("/posts/{post}", page()),
            )
            .page("/users/{name}", page())
    }

    #[test]
    fn nesting_matchers_and_fallbacks() {
        assert_eq!(resolve(app(), "/").0, "/");
        let (route, params, _) = resolve(app(), "/users/1/posts/2?x=1");
        assert_eq!(route, "/users/{id=int}/posts/{post}");
        assert_eq!(params.len(), 2);
        assert_eq!(resolve(app(), "/users/1/").0, "/users/{id=int}");
        assert_eq!(resolve(app(), "/users/ferris").0, "/users/{name}");
        assert_eq!(resolve(app(), "/about").1, [("lang".into(), None)]);

        let (route, params, _) = resolve(app(), "/users/1/nope");
        assert_eq!(route, "/users/{id=int}/{*__not_found}");
        assert_eq!(params, [("id".into(), Some("1".into()))]);
        assert_eq!(resolve(app(), "/nope").0, "/{*__not_found}");
    }

    #[test]
    fn layout_resets() {
        use crate::page::Layout;
        let layout = || Layout::new(|_: &mut egui::Ui, _: crate::page::Outlet<'_>| {});
        let router = || {
            Router::new().layout(layout()).nest(
                "/a",
                Router::new().named("a").layout(layout()).nest(
                    "/b",
                    Router::new()
                        .layout(layout())
                        .page("/", page())
                        .page("/root", page().reset_layouts())
                        .page("/to-a", page().reset_layouts_to("a")),
                ),
            )
        };
        assert_eq!(resolve(router(), "/a/b").2, 3);
        assert_eq!(resolve(router(), "/a/b/root").2, 1);
        assert_eq!(resolve(router(), "/a/b/to-a").2, 2);
    }

    #[test]
    fn merged_routers_leave_404s_to_their_parent() {
        let error = |_: &mut egui::Ui, _: &crate::RouteError| {};
        let table = Router::<()>::new()
            .page("/", page())
            .merge(Router::new().error(error).page("/merged", page()))
            .nest(
                "/nested",
                Router::new().merge(Router::new().error(error).page("/", page())),
            )
            .into_table();
        assert_eq!(table.fallbacks.len(), 2);
        let (entry, _) = table.resolve("/nope");
        assert!(entry.levels.iter().all(|level| level.error.is_none()));
        let (entry, req) = table.resolve("/nested/nope");
        assert_eq!(req.route(), "/nested/{*__not_found}");
        assert_eq!(entry.levels.len(), 2);
    }

    #[test]
    #[should_panic(expected = "conflicting routes")]
    fn conflicts() {
        Router::<()>::new()
            .page("/blog/{a}", page())
            .nest("/blog", Router::new().page("/{b}", page()))
            .into_table();
    }

    #[test]
    #[should_panic(expected = "unknown matcher")]
    fn unknown_matcher() {
        Router::<()>::new().page("/{id=int}", page()).into_table();
    }
}
