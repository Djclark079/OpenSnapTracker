# Architecture

OpenSnapTracker uses Electron for the v1 desktop shell and Rust for the core tracker engine. State reading, normalization, reconciliation, metadata, storage, image caching, and diagnostics should stay in Rust where practical.

Electron was selected from spike evidence, not preference: it demonstrated the required KDE Wayland/XWayland overlay behavior with fewer compatibility problems than Tauri.

## Pipeline

```text
State files
  -> Rust Snapshot Reader
  -> Rust Normalizer
  -> Rust Reconciliation
  -> Structured Events
  -> Current Match State
  -> Electron Overlay Views
```

The snapshot reader treats files as externally owned and possibly mid-write. It retries parse failures with bounded backoff and never repairs malformed JSON.

## Snapshot Observation

The first normalizer layer lives in `state-reader` and produces conservative snapshot observations:
- resolves JSON.NET-style `$id` and `$ref` references before reading players, zones, and cards
- identifies local player and opponent from `RemoteGame.ClientGameInfo`
- reports raw zones alongside coarse domain zones
- treats hidden cards as card instances with no `CardDefId`
- diffs consecutive observations for appeared cards, card-definition reveals, and zone changes

This layer intentionally does not decide whether raw `Graveyard` means destroyed or discarded. That distinction belongs in reconciliation, using transition context and fixture-backed evidence.

## Reconciliation

The first reconciliation pass consumes pairs of `SnapshotObservation` values and emits conservative domain events:
- `MatchStarted` for the first in-match observation
- `CardInstanceObserved` for observed card instances
- `CardDrawn` for deck-to-hand movement
- `CardPlayed` for hand-to-board movement
- `CardRevealed` when a previously hidden card gains a `CardDefId`
- `CardDiscarded` for hand-to-raw-`Graveyard` movement
- `CardDestroyed` for board-to-raw-`Graveyard` movement
- `CardGenerated` for newly appeared card instances with no observed original-deck reference
- `UnknownTransitionObserved` for raw movements that still need more evidence

Other raw `Graveyard` paths, transform, merge, transfer, and theft events remain intentionally unclaimed until targeted fixtures prove the state-file signal.

## Overlay Projection

`state-reader` also exposes a lightweight overlay projection for development replay. It reports player/opponent deck, hand, board, destroyed, discarded, removed, and unknown-transition counts plus known/hidden card visibility and whether an observed original-deck candidate has left the deck. A stateful projector applies reconciliation events so raw `Graveyard` cards can land in destroyed/discarded buckets when the transition is known. This is a bridge toward the final overlay state, not the finished stable 12-slot layout model.

## Desktop Shell

Electron owns the local desktop shell:
- two transparent overlay windows
- global hotkeys
- passthrough/edit/interactive modes
- persistent geometry
- AppImage packaging

The Rust core should be callable from Electron through a sidecar process, IPC boundary, or other small integration layer chosen during the next implementation milestone. The core should not depend on Electron APIs.

## Domain Principles

- Original deck slots stay stable.
- Supplemental cards never enter the original 12-card grid.
- Destroyed, discarded, transformed, merged, returned, confirmed removed, and unknown transitions remain distinct internally.
- Opponent cards can be unknown, known, or inferred.
- Unknown snapshot fields are preserved where practical for diagnostics.

## Storage

SQLite is used for card definitions, catalogue revision, image cache metadata, settings, overlay geometry, optional diagnostic snapshots, and future match events. Migrations are required from the beginning.

Image files are stored on disk, not as SQLite blobs.

## Privacy

The application reads local game state files only, sends no match data by default, has no telemetry, and requires no account. Raw captures can contain identifiers and must not be committed without redaction.
