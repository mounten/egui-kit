//! Loading a route: hooks first, then the loads of its layouts and page.
//!
//! A load only reruns when something it used changed. What it used is
//! recorded while it runs (see [`Deps`]); a navigation keeps the previous data of every layout
//! and page whose path, query or params (as far as it read them) are unchanged, that wasn't
//! [invalidated](Invalidation) and whose parent data (if it asked for it) wasn't reloaded.
//! The loads that do run, run concurrently; [`Parent`] waits for the layouts above.

use core::any::Any;
use core::cell::RefCell;
use core::future::poll_fn;
use core::task::Poll;
use std::rc::Rc;

use crate::extract::parent::{ParentHandle, Slot, Slots};

use crate::error::{Rejection, RouteError};
use crate::exec;
use crate::page::{ErasedHandler, Loader, LocalBoxFuture, NodeId};
use crate::request::{Deps, Request};
use crate::router::{RouteEntry, Target};

/// The data a layout or page loaded, and what it depended on.
/// Loaded data, shared between the page on screen and a navigation that keeps it.
pub(crate) type DataCell = Rc<RefCell<Box<dyn Any>>>;

#[derive(Clone)]
pub(crate) struct Loaded {
    pub(crate) node: NodeId,
    pub(crate) data: DataCell,
    pub(crate) deps: Deps,
    /// The request it was loaded with, to compare with the next navigation.
    pub(crate) request: Request,
}

/// Which loads must rerun regardless of their dependencies.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) enum Invalidation {
    #[default]
    None,
    Keys(Vec<String>),
    All,
}

impl Invalidation {
    pub(crate) fn hits(&self, keys: &[String]) -> bool {
        match self {
            Self::None => false,
            Self::Keys(invalidated) => keys.iter().any(|key| invalidated.contains(key)),
            Self::All => true,
        }
    }

    pub(crate) fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::All, _) | (_, Self::All) => Self::All,
            (Self::None, other) | (other, Self::None) => other,
            (Self::Keys(mut a), Self::Keys(b)) => {
                a.extend(b);
                Self::Keys(a)
            }
        }
    }
}

/// The result of loading a route.
pub(crate) struct Outcome {
    /// Per level of the route; `None` for levels without layout, or that didn't load.
    pub(crate) layouts: Vec<Option<Loaded>>,
    pub(crate) page: Option<Loaded>,
    /// The outermost level that failed, and why (`layouts.len()` = the page itself).
    pub(crate) failure: Option<(usize, Rejection)>,
}

/// What to do for one layout or the page.
enum Step<S> {
    Reuse(Loaded),
    Run {
        node: NodeId,
        load: ErasedHandler<S>,
        params: usize,
    },
    /// A level without layout.
    Nothing,
    NotFound,
}

/// Loading one route, for a navigation or a preload.
pub(crate) struct Job<S> {
    pub(crate) entry: Rc<RouteEntry<S>>,
    pub(crate) request: Request,
    /// Until it's done.
    future: Option<LocalBoxFuture<Outcome>>,
    outcome: Option<Outcome>,
}

impl<S: Clone + 'static> Job<S> {
    /// Plans which loads to rerun against the data of the page on screen (`previous`), and
    /// starts the hooks and loads.
    pub(crate) fn start(
        entry: Rc<RouteEntry<S>>,
        request: Request,
        state: &S,
        previous: &[&Loaded],
        invalidation: &Invalidation,
    ) -> Self {
        let steps = plan(&entry, &request, previous, invalidation);
        let future = Box::pin(run(
            Rc::clone(&entry),
            request.clone(),
            state.clone(),
            steps,
        ));
        Self {
            entry,
            request,
            future: Some(future),
            outcome: None,
        }
    }

    /// Drives the job; `true` once its outcome is ready.
    pub(crate) fn poll(&mut self, ctx: &egui::Context) -> bool {
        if let Some(future) = &mut self.future
            && let Poll::Ready(outcome) = exec::poll_once(ctx, future.as_mut())
        {
            self.future = None;
            self.outcome = Some(outcome);
        }
        self.outcome.is_some()
    }

    pub(crate) fn take_outcome(&mut self) -> Option<Outcome> {
        self.outcome.take()
    }
}

fn plan<S>(
    entry: &RouteEntry<S>,
    request: &Request,
    previous: &[&Loaded],
    invalidation: &Invalidation,
) -> Vec<Step<S>> {
    let mut parent_reran = false;
    let mut step = |loader: &Loader<S>, params: usize| {
        let request = request.for_handler(params, None);
        let reusable = previous.iter().find(|loaded| {
            loaded.node == loader.id
                && !invalidation.hits(&loader.keys)
                && !(loaded.deps.parent && parent_reran)
                && loaded.request.same_for(loaded.deps, &request)
        });
        if let Some(loaded) = reusable {
            Step::Reuse((*loaded).clone())
        } else {
            parent_reran = true;
            Step::Run {
                node: loader.id,
                load: Rc::clone(&loader.load),
                params,
            }
        }
    };

    let mut steps: Vec<Step<S>> = entry
        .levels
        .iter()
        .map(|level| match &level.layout {
            Some(layout) => step(&layout.loader, level.params),
            None => Step::Nothing,
        })
        .collect();
    steps.push(match &entry.target {
        Target::Page(page) => step(&page.loader, usize::MAX),
        Target::NotFound => Step::NotFound,
    });
    steps
}

/// A load in flight.
struct Task {
    index: usize,
    request: Request,
    future: Option<LocalBoxFuture<Result<Box<dyn Any>, Rejection>>>,
    result: Option<Result<DataCell, Rejection>>,
}

async fn run<S: 'static>(
    entry: Rc<RouteEntry<S>>,
    request: Request,
    state: S,
    mut steps: Vec<Step<S>>,
) -> Outcome {
    let hook_failure = run_hooks(&entry, &request, &state).await;
    if let Some((index, rejection)) = &hook_failure {
        // Nothing below a failed hook loads. Above it, layouts still load to wrap the error page,
        // unless the hook redirects anyway.
        let keep = if matches!(rejection, Rejection::Redirect(_)) {
            0
        } else {
            *index
        };
        for step in &mut steps[keep..] {
            *step = Step::Nothing;
        }
    }

    let slots: Slots = Rc::new(RefCell::new(
        steps
            .iter()
            .map(|step| match step {
                Step::Reuse(loaded) => Slot::Ready(Rc::clone(&loaded.data)),
                Step::Run { .. } => Slot::Waiting,
                Step::Nothing | Step::NotFound => Slot::Empty,
            })
            .collect(),
    ));
    let mut tasks: Vec<Task> = steps
        .iter()
        .enumerate()
        .filter_map(|(index, step)| {
            let Step::Run { load, params, .. } = step else {
                return None;
            };
            let mut task_request = request.for_handler(*params, None);
            task_request.parent = Some(ParentHandle::new(
                Rc::clone(&slots),
                index,
                Rc::clone(&task_request.deps),
            ));
            let future = load(&task_request, &state);
            Some(Task {
                index,
                request: task_request,
                future: Some(future),
                result: None,
            })
        })
        .collect();
    join(&mut tasks, &slots).await;

    let mut loaded: Vec<Option<Loaded>> = Vec::with_capacity(steps.len());
    let mut failure = hook_failure;
    let mut fail = |index: usize, rejection: Rejection| {
        if failure
            .as_ref()
            .is_none_or(|(outermost, _)| index < *outermost)
        {
            failure = Some((index, rejection));
        }
    };
    let mut tasks = tasks.into_iter();
    for (index, step) in steps.into_iter().enumerate() {
        loaded.push(match step {
            Step::Reuse(reused) => Some(reused),
            Step::Nothing => None,
            Step::NotFound => {
                fail(
                    index,
                    RouteError::not_found(format!("not found: {}", request.untracked_path()))
                        .into(),
                );
                None
            }
            Step::Run { node, .. } => {
                let task = tasks
                    .next()
                    .filter(|task| task.index == index)
                    .expect("one task per run step");
                match task.result.expect("joined") {
                    Ok(data) => Some(Loaded {
                        node,
                        data,
                        deps: task.request.deps.get(),
                        request: task.request,
                    }),
                    Err(rejection) => {
                        fail(index, rejection);
                        None
                    }
                }
            }
        });
    }

    let page = loaded.pop().flatten();
    Outcome {
        layouts: loaded,
        page,
        failure,
    }
}

/// Runs the hooks, outermost first: they guard everything below them. Returns the first failure.
async fn run_hooks<S>(
    entry: &RouteEntry<S>,
    request: &Request,
    state: &S,
) -> Option<(usize, Rejection)> {
    for (index, level) in entry.levels.iter().enumerate() {
        let request = request.for_handler(level.params, None);
        for hook in &level.hooks {
            if let Err(rejection) = hook(&request, state).await {
                return Some((index, rejection));
            }
        }
    }
    None
}

/// Polls all tasks until they are done. Whenever one finishes, the others are polled again
/// right away, since they may be waiting for it through [`Parent`].
async fn join(tasks: &mut [Task], slots: &Slots) {
    poll_fn(|cx| {
        loop {
            let mut progressed = false;
            for task in tasks.iter_mut() {
                let Some(future) = &mut task.future else {
                    continue;
                };
                let Poll::Ready(result) = future.as_mut().poll(cx) else {
                    continue;
                };
                task.future = None;
                progressed = true;
                let result = result.map(|data| Rc::new(RefCell::new(data)));
                slots.borrow_mut()[task.index] = match &result {
                    Ok(data) => Slot::Ready(Rc::clone(data)),
                    Err(_) => Slot::Failed,
                };
                task.result = Some(result);
            }
            if tasks.iter().all(|task| task.future.is_none()) {
                return Poll::Ready(());
            }
            if !progressed {
                return Poll::Pending;
            }
        }
    })
    .await;
}
