# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
