# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0](https://github.com/mounten/egui-kit/compare/v0.2.0...v0.3.0) - 2026-10-12

### Added

- *(remote)* [**breaking**] replace remote functions with owned `Query` and `Command` handles ([#30](https://github.com/mounten/egui-kit/pull/30))
- *(router)* add `RouterApp::matches` to resolve urls ([#26](https://github.com/mounten/egui-kit/pull/26))
- *(history)* implement `History` for boxed histories ([#25](https://github.com/mounten/egui-kit/pull/25))
- *(extract)* add `Nav::replace_url` ([#23](https://github.com/mounten/egui-kit/pull/23))
- *(extract)* read and write `PersistentState` outside of views ([#22](https://github.com/mounten/egui-kit/pull/22))
- *(extract)* add `PageStateFrom` and `SnapshotFrom` ([#21](https://github.com/mounten/egui-kit/pull/21))
- *(transition)* [**breaking**] tell effects which navigation they animate ([#20](https://github.com/mounten/egui-kit/pull/20))
- *(extract)* make `Nav::follow` public ([#18](https://github.com/mounten/egui-kit/pull/18))
- *(extract)* add `LayoutData` to read a layout's data in views ([#17](https://github.com/mounten/egui-kit/pull/17))
- *(remote)* run futures like commands, from views or anywhere ([#16](https://github.com/mounten/egui-kit/pull/16))
- *(remote)* add keyed command handles ([#15](https://github.com/mounten/egui-kit/pull/15))
- *(remote)* add `CommandHandle::take_error` and `take_result` ([#14](https://github.com/mounten/egui-kit/pull/14))
- *(remote)* add `QueryState::version` and `QueryState::rc` ([#13](https://github.com/mounten/egui-kit/pull/13))
- *(remote)* keep unread queries for a while ([#12](https://github.com/mounten/egui-kit/pull/12))
- *(remote)* refetch queries periodically ([#11](https://github.com/mounten/egui-kit/pull/11))
- *(task)* expose the executor to poll futures outside the router ([#10](https://github.com/mounten/egui-kit/pull/10))

### Documentation

- document redirects in the history, goto to the current url and query cancellation ([#27](https://github.com/mounten/egui-kit/pull/27))
- explain how to use Tokio-based clients natively ([#9](https://github.com/mounten/egui-kit/pull/9))

### Fixed

- *(extract)* make `Nav::is_active` follow shallow routing ([#24](https://github.com/mounten/egui-kit/pull/24))
- *(transition)* make the outgoing page non-interactive ([#19](https://github.com/mounten/egui-kit/pull/19))
- *(router)* don't add a 404 fallback for merged routers ([#8](https://github.com/mounten/egui-kit/pull/8))
- *(navigation)* keep a pending navigation when asked to go to its url again ([#7](https://github.com/mounten/egui-kit/pull/7))

## [0.2.0](https://github.com/mounten/egui-kit/compare/v0.1.0...v0.2.0) - 2026-10-11

### Added

- *(remote)* [**breaking**] new (async) queries and commands available for page ui methods ([#6](https://github.com/mounten/egui-kit/pull/6))
- *(history)* showing hovered links in the browser ([#4](https://github.com/mounten/egui-kit/pull/4))

## [0.1.0](https://github.com/mounten/egui-kit/releases/tag/v0.1.0) - 2026-10-11

### Added

Initial release.

- `Router` with nested and merged routers, path parameters, parameter matchers, optional
  parameters and rest parameters.
- Pages with an optional async `load` and a view, and layouts that wrap the pages of a router.
- Extractors for loads and hooks (`Path`, `Query`, `State`, `Parent`, `Request`) and for views
  (`Nav`, `State`, `Scroll`, `Navigation`, `Request`).
- Hooks that run before the loads of a router's pages, error pages, and redirects.
- Loads only rerun when their inputs change, can be invalidated by key, and are preloaded when a
  link is hovered.
- View state that lives with the page (`PageState`), the history entry (`Snapshot`) or across
  app restarts (`PersistentState`).
- Shallow routing with `Nav::push_state` and `HistoryState`.
- Blocking navigation while a page has unsaved changes.
- View transitions between pages.
- Browser history on the web, and in-memory history with mouse back/forward buttons and Alt+←/→
  natively.
