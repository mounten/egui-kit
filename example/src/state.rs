//! The app state. Loads and hooks extract a clone of all of it (`State<AppState>`) or of just the
//! part they need (`State<Db>`, `State<Session>`), thanks to the `FromRef` impls below; views
//! borrow it (`State<&AppState>`, `State<&Db>`). Everything shared is behind `Rc`, so the clones
//! are cheap and see the same data.

use core::cell::{Cell, RefCell};
use std::rc::Rc;

use egui_kit::ViewTransition;
use egui_kit::extract::FromRef;
use serde::{Deserialize, Serialize};

/// Loads that read posts declare this dependency; publishing a post invalidates it.
pub const POSTS: &str = "app:posts";

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub session: Session,
    /// Counted by an `after_navigate` callback.
    pub navigations: Rc<Cell<u32>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            db: Db::seed(),
            session: Session::default(),
            navigations: Rc::default(),
        }
    }
}

/// Picked in the top bar. A preference, so it is a `PersistentState`: it survives restarts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransitionStyle {
    /// The one from the SvelteKit blog post.
    #[default]
    Slide,
    Fade,
    Push,
    Off,
}

impl TransitionStyle {
    pub const ALL: [Self; 4] = [Self::Slide, Self::Fade, Self::Push, Self::Off];

    pub fn view_transition(self) -> ViewTransition {
        match self {
            Self::Slide => ViewTransition::slide(),
            Self::Fade => ViewTransition::fade(),
            Self::Push => ViewTransition::push(),
            Self::Off => ViewTransition::fade().duration(0.0),
        }
    }
}

impl FromRef<AppState> for Db {
    fn from_ref(state: &AppState) -> &Self {
        &state.db
    }
}

impl FromRef<AppState> for Session {
    fn from_ref(state: &AppState) -> &Self {
        &state.session
    }
}

#[derive(Clone, Debug)]
pub struct Post {
    pub slug: String,
    pub title: String,
    pub author_id: u32,
    pub tags: Vec<String>,
    pub body: String,
}

#[derive(Clone, Debug)]
pub struct Photo {
    pub id: u32,
    pub title: String,
    pub color: egui::Color32,
}

#[derive(Clone, Debug)]
pub struct Author {
    pub id: u32,
    pub name: &'static str,
}

/// A stand-in for a real backend.
#[derive(Clone)]
pub struct Db {
    posts: Rc<RefCell<Vec<Post>>>,
    authors: Rc<Vec<Author>>,
    photos: Rc<Vec<Photo>>,
}

impl Db {
    fn seed() -> Self {
        let post = |slug: &str, title: &str, author_id, tags: &[&str], body: &str| Post {
            slug: slug.into(),
            title: title.into(),
            author_id,
            tags: tags.iter().map(|tag| tag.to_string()).collect(),
            body: body.into(),
        };
        let posts = vec![
            post(
                "hello-egui-kit",
                "Hello, egui-kit",
                1,
                &["egui", "routing"],
                "egui-kit brings SvelteKit-style routing to egui: pages with typed load functions, \
                 nested layouts, hooks, error pages and browser history.",
            ),
            post(
                "typed-params",
                "Typed path and query parameters",
                2,
                &["routing", "serde"],
                "Path<T> and Query<T> deserialize parameters with serde. Matchers like {id=int} \
                 pick between routes: /authors/1 and /authors/ferris are different pages.",
            ),
            post(
                "layouts-all-the-way-down",
                "Layouts all the way down",
                1,
                &["egui", "layouts"],
                "Every nested router can have a layout, an error page and hooks. Layouts may load \
                 data too: the sidebar on the left comes from a layout load, and the post list \
                 reads it with Parent.",
            ),
        ];
        let authors = vec![
            Author {
                id: 1,
                name: "Ferris",
            },
            Author {
                id: 2,
                name: "Corro",
            },
        ];
        let photos = (1..=24)
            .map(|id| Photo {
                id,
                title: format!("Photo #{id}"),
                color: egui::ecolor::Hsva::new(id as f32 / 24.0, 0.6, 0.8, 1.0).into(),
            })
            .collect();
        Self {
            posts: Rc::new(RefCell::new(posts)),
            authors: Rc::new(authors),
            photos: Rc::new(photos),
        }
    }

    pub fn posts(&self) -> Vec<Post> {
        self.posts.borrow().clone()
    }

    pub fn post(&self, slug: &str) -> Option<Post> {
        self.posts
            .borrow()
            .iter()
            .find(|post| post.slug == slug)
            .cloned()
    }

    /// Adds a post and returns its slug.
    pub fn publish(&self, title: &str, body: &str, author_id: u32) -> String {
        let slug: String = title
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect();
        self.posts.borrow_mut().push(Post {
            slug: slug.clone(),
            title: title.into(),
            author_id,
            tags: vec!["new".into()],
            body: body.into(),
        });
        slug
    }

    pub fn photos(&self) -> &[Photo] {
        &self.photos
    }

    pub fn photo(&self, id: u32) -> Option<&Photo> {
        self.photos.iter().find(|photo| photo.id == id)
    }

    pub fn author(&self, id: u32) -> Option<&Author> {
        self.authors.iter().find(|author| author.id == id)
    }

    pub fn author_by_name(&self, name: &str) -> Option<&Author> {
        self.authors
            .iter()
            .find(|author| author.name.eq_ignore_ascii_case(name))
    }

    pub fn tags(&self) -> Vec<String> {
        let mut tags: Vec<_> = self
            .posts
            .borrow()
            .iter()
            .flat_map(|post| post.tags.clone())
            .collect();
        tags.sort_unstable();
        tags.dedup();
        tags
    }
}

/// The logged in user.
#[derive(Clone, Default)]
pub struct Session(Rc<RefCell<Option<String>>>);

impl Session {
    pub fn user(&self) -> Option<String> {
        self.0.borrow().clone()
    }

    pub fn login(&self, name: String) {
        *self.0.borrow_mut() = Some(name);
    }

    pub fn logout(&self) {
        *self.0.borrow_mut() = None;
    }
}
