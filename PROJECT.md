# Marvel Snap Linux Tracker

## Document purpose

This file is the canonical product reference for the project.

It records:
- product goals
- supported platforms
- settled design decisions
- terminology
- scope boundaries
- unresolved research questions
- acceptance criteria

Implementation plans belong in PLANS.md.
Technical details belong in docs/architecture.md.
Agent working instructions belong in AGENTS.md.

## Product vision

Build a Linux-native real-time Marvel Snap deck tracker that works with the game running through Steam and Proton.

The application provides two lightweight overlay windows:
- Player Deck
- Opponent Deck

The application reads local game-state files only. It does not inject into the game process or modify game files.

## Primary target

- Linux desktop
- KDE Plasma Wayland as the initial reference environment
- Marvel Snap running through Steam/Proton
- Overlay running through XWayland/X11 when required
- AppImage as the primary distribution format

Other desktop environments and distributions should remain possible, but they are not the first compatibility target.

## Product principles

- Read-only interaction with game files
- No game-process injection
- No telemetry
- No account required
- Offline-capable after metadata and art are cached
- Stable and legible overlays
- Graceful handling of unknown game-state behavior
- No fabricated certainty when card transitions cannot be proven

## Overlay windows

The application has two independently movable and resizable windows.

### Player Deck

Shows the 12 original cards in the player's starting deck.

States:
- In deck: normal artwork
- Drawn or otherwise no longer in deck: darkened and desaturated

Cards remain in stable grid positions.

### Opponent Deck

Shows 12 original opponent deck slots.

States:
- Unknown: card-back placeholder
- Known but unplayed: normal artwork
- Played or otherwise consumed: darkened and desaturated

Known cards remain in stable grid positions once assigned.

## Layout

Version one uses:
- 3 columns
- 4 rows
- fixed original-deck grid
- compact header
- compact footer
- responsive resizing with minimum dimensions

Future versions may add:
- horizontal layouts
- alternate card density
- more layout controls

## Footer counters

Both overlays show:

1. Deck
2. Hand
3. Destroyed
4. Discarded
5. Removed

Destroyed, Discarded, and Removed are clickable.

Clicking a counter replaces the normal deck grid with an expanded zone view in the same window.

Expanded views:
- include a back button
- retain the footer
- use a card grid
- scroll vertically when necessary

## Supplemental cards

Cards that were not part of the original starting deck do not enter the original 12-card grid.

Examples:
- generated cards
- copied cards
- stolen cards
- transferred cards
- cards added to hand
- cards added to deck
- transformed results

These appear in a separate supplemental area.

## Interaction modes

### Interactive

Default mode.

- card hover works
- footer buttons work
- window position is locked

### Passthrough

- all pointer input passes through to the game
- tooltips and buttons are disabled
- toggled by global hotkey

### Edit

- windows may be moved and resized
- minimum size is enforced
- geometry is persisted

## Card hover tooltip

Hovering a card shows:
- name
- base cost
- base power
- canonical ability text

The tracker does not display temporary in-match cost or power changes.

## Card art

The project metadata service returns an upstream image URL.

The client:
- downloads art directly from the upstream host
- caches it locally
- uses the full URL as the asset identity
- treats a changed URL as a new revision
- may store Last-Modified for conditional revalidation
- uses placeholders when art is unavailable

The project server does not act as an image CDN by default.

Direct automated image fetching remains subject to provider permission and licensing review.

## Metadata system

The client must not contain the private oanor API key.

A small project-controlled metadata service will:
- hold the upstream API key
- fetch card metadata
- normalize records
- publish catalogue revisions
- serve deltas to clients
- retain the last known good catalogue during upstream outages

The client will:
- ship with a seeded SQLite database
- work offline
- request changes after its current catalogue revision
- apply updates transactionally

## State tracking architecture

Expected pipeline:

Game state files
→ Snapshot Reader
→ Snapshot Normalizer
→ Reconciliation Engine
→ Structured Events
→ Current Match State
→ Overlay Views

The reader must:
- tolerate partially written files
- retry boundedly
- never repair arbitrary malformed JSON
- remain read-only
- preserve diagnostic information
- avoid blocking the UI

## Domain terminology

### Original deck

Cards belonging to the starting deck or starting deck expansion.

### Supplemental card

A card introduced after the starting deck was established.

### Removed

UI category for a card no longer accounted for in normal zones.

Internally, preserve finer distinctions:
- removed confirmed
- transformed
- merged
- returned
- unknown transition

Do not classify every unexplained disappearance as Removed.

### Card definition

Canonical metadata for a card.

### Card instance

A particular in-match occurrence of a card.

## Stats and history

Match statistics are low priority.

The architecture should allow future event persistence without requiring a major rewrite.

Version one does not require:
- win/loss analytics
- deck performance reports
- collection tracking
- cloud accounts
- social features

## Shell decision

Electron and Tauri 2 remain candidates.

The shell must be selected using an overlay technology spike, not preference alone.

The spike must test:
- two transparent windows
- always-on-top behavior
- hover interaction
- click-through
- global hotkeys
- resizing
- persistent geometry
- multi-monitor behavior
- XWayland startup
- AppImage packaging

## Known Linux requirement

Exiled Exchange 2 requires the following on tested KDE Wayland systems:

Environment:
XDG_SESSION_TYPE=x11

Arguments:
--ozone-platform=x11
--force-device-scale-factor=1

The project must determine which of these are actually required.

Any required behavior must be built into packaging and launch integration.

## Initial acceptance gates

### Gate 1: Overlay feasibility

At least one shell can provide two stable, interactive, independently positioned overlay windows under KDE Wayland via XWayland.

### Gate 2: State visibility

The capture utility can collect enough snapshots to determine how cards, card instances, zones, and transitions are represented.

### Gate 3: Reconciliation

Fixture-driven tests can reproduce common transitions:
- draw
- play
- destroy
- discard
- generate
- transfer
- transform
- match start
- match end

### Gate 4: Metadata and art

The client can:
- read seeded metadata
- apply a catalogue delta
- fetch art
- cache art
- render offline from cache

## Explicitly out of scope for initial versions

- game injection
- game-file modification
- board reconstruction
- live temporary stat tracking
- automatic deck recommendations
- account system
- cloud sync
- telemetry
- monetization
- mobile support
- Windows-first design

## Open research questions

- Does the game expose stable card-instance IDs?
- How are transformed and merged cards represented?
- How reliably can Removed be distinguished from unknown transitions?
- Which XWayland flags are actually required?
- Which shell is more reliable for the overlay?
- Are upstream image URLs contractually permitted for client-side fetching?
- How are unusual decks such as Thanos and Arishem represented?
- What information is available for the opponent hand and original deck?

## Reference material

- helper-for-marvel-snap
- Exiled Exchange 2
- oanor Marvel Snap API
- Untapped.gg screenshots used only as functional design references

## Change policy

Changes to settled product behavior should update this file in the same pull request.

Changes to domain semantics require:
- an explanation
- fixture-backed tests where applicable
- updates to architecture documentation