//! Describing navigations: where from, where to, how.

/// How the page on screen was reached. An extractor for views,
/// and passed to [`after_navigate`](crate::RouterApp::after_navigate) callbacks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Navigation {
    /// The url of the previous page, `None` for the first page.
    pub from: Option<String>,
    pub to: String,
    pub kind: NavigationKind,
    pub direction: Direction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationKind {
    /// The first page of the app.
    Enter,
    /// [`Nav::goto`](crate::extract::Nav::goto) or a link (also when it was redirected).
    Goto,
    /// [`Nav::replace`](crate::extract::Nav::replace), or a redirect after back/forward.
    Replace,
    /// Back/forward.
    Popstate,
    /// [`Nav::invalidate`](crate::extract::Nav::invalidate): the same page was reloaded.
    Invalidate,
}

/// Where in the history the new page is, relative to the previous one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Forward,
    Back,
    /// Same entry (replace, invalidate) or unknown (entries from before a browser reload).
    None,
}

impl Direction {
    /// `1.0` forward, `-1.0` back, `0.0` otherwise. Handy to mirror slide animations.
    pub fn sign(self) -> f32 {
        match self {
            Self::Forward => 1.0,
            Self::Back => -1.0,
            Self::None => 0.0,
        }
    }
}

/// Options for [`Nav::goto_with`](crate::extract::Nav::goto_with).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GotoOptions {
    pub(crate) replace_state: bool,
    pub(crate) no_scroll: bool,
    pub(crate) invalidate_all: bool,
}

impl GotoOptions {
    /// Replace the current history entry instead of adding one.
    pub fn replace_state(mut self) -> Self {
        self.replace_state = true;
        self
    }

    /// Keep the scroll position of the current page (see [`Scroll`](crate::extract::Scroll)).
    pub fn no_scroll(mut self) -> Self {
        self.no_scroll = true;
        self
    }

    /// Rerun every `load`, even those whose dependencies didn't change.
    pub fn invalidate_all(mut self) -> Self {
        self.invalidate_all = true;
        self
    }
}
