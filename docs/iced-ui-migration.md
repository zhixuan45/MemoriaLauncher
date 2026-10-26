# iced UI migration and responsiveness

Based on upstream `9cd898865c32e636801fe888b6ccde8ae394eb22` and the four supplied design references. Slint source and artwork remain as migration references; the desktop binary uses iced only.

## Implemented

- The shell now uses the dark project workspace design; Home, chat, downloads and settings remain reachable through the persistent sidebar. Navigating clears modal/drag/layout-edit state without discarding the download filters, selected version or AI input.
- Downloads: five resource categories, search, release/snapshot and loader filters, date ordering, selection, four detail tabs, banner controls and loading/error/empty states. Catalogue loading uses the existing asynchronous `CatalogProvider` contract, once on first entry (or explicitly on refresh).
- AI: a minimal opening composer, project conversations, session-only history, clear, failure retry and opt-in file/log context. The former suggestions, examples, selectors, unsupported search controls and task panel were removed. Provider configuration stays in Settings.
- Settings: seven navigable tabs, five general-settings cards, keyword search and an explicit explanation for categories whose configuration service is not connected.
- The default homepage is now a project dashboard with instance statistics, launch and conversation entries. The previous home canvas, selection/reordering and auxiliary cards remain available through the layout editor. Account and queue services are reused.

## Shared motion

`motion::fade` and `motion::pop` wrap existing widgets without changing their layout. The renderer applies opacity to panels, borders, shadows, text, images and SVGs on both existing backends. Pages crossfade over 180 ms. Each destination card independently pops from 94% to 100% scale, moving upward 12 px and fading in over 50 ms, starting together on navigation. The outgoing page's cards stay settled. Dialogs use the same pop over 220 ms and fade out over 150 ms. All use cubic ease-out.

`motion::Toggle` slides the round thumb between left/right positions and interpolates track/thumb colors over 120 ms in settings, retaining native toggler input. The value changes immediately, and rapid clicks reverse from the current position and color. Restoring default settings also animates affected switches. Labels remain at full opacity; switches share the application's animation timer, which stops when all transitions settle.

Animation ticks are subscribed only while a transition is active. Repeated dismissal does not restart the exit, and reopening the same dialog reverses from its current progress. The dialog and its input barrier remain until the exit completes. Widget hit-testing follows the pop transform, while the stable shell tree retains focus and scroll state across dialog transitions.

Optional transition snapshots capture the start, middle and end of page changes and dialog entry/exit:

```sh
ICED_TEST_BACKEND=tiny-skia MEMORIA_UI_CAPTURE_DIR=/tmp/memoria-motion \
  cargo test -p desktop capture_motion -- --ignored --test-threads=1
```

## Responsiveness changes

The previous manifest enabled only `tiny-skia`, which means CPU rasterization even when a GPU is available. Enable `wgpu` alongside the software fallback. iced selects the GPU compositor first unless overridden by the environment. `ICED_BACKEND=tiny-skia` remains available for troubleshooting. Linux X11/Wayland features are explicit; the previous no-default-features manifest did not compile the Linux softbuffer backend.

The previous application subscribed to every runtime input event, including ordinary cursor motion, and rebuilt all SVG strings/handles during every view call. Now:

- Normal operation subscribes only to resize, focus loss and Escape. Widgets handle their own pointer events.
- Cursor motion and mouse release reach application state only while editing the home layout. Captured releases remain subscribed so drags terminate over interactive controls.
- SVG handles are reused by icon and RGBA palette value; size is applied by the widget. PNG handles were already cached and remain so. This avoids formatting, allocation and hashing work; the old SVG IDs were content hashes, so this is not a claim that textures were re-uploaded every frame.
- Scrollbars reserve a narrow gutter instead of covering list buttons.
- Removed the repository-wide Windows-specific Cargo target path; use `CARGO_TARGET_DIR` locally if desired.

`event::listen_with` in iced already excludes redraw notifications. This change does not claim to fix an infinite redraw loop. The reported inability to navigate was not independently reproduced in headless widget hit-testing; missing download/AI views and excessive pointer-driven updates have been addressed. Windows-specific focus, scaling and driver behavior still need a native run.

## Backend boundaries

The desktop now scans local instances, persists Microsoft accounts using the Windows credential vault, launches through the existing core runtime, and reads Mojang's official version manifest. See [Desktop data sources](real-data-integration.md) for configuration and validation boundaries. Sample providers remain available as test fixtures.

Modrinth mod/modpack/resource-pack/shader catalogues now support search, version/loader filters, pagination and version-file/dependency inspection. Image/text/PDF attachments are supported; third-party installation and AI tool execution remain unavailable. Their actions explain the missing service and never fabricate successful work. Vanilla installation now downloads verified files through the configured source and concurrency limit. Game/download/AI/account configuration is persistent; other general switches retain their session-only behavior.

## Verification

```sh
cargo check --workspace --locked
ICED_TEST_BACKEND=tiny-skia cargo test --locked -p desktop -p mc-core -- --test-threads=1
cargo run --release --locked -p desktop
```

The desktop tests exercise actual widget hit targets, not just direct `Navigate` messages: all four source/destination combinations at 1024×680 and 1536×921, download/settings tabs, modal blocking, retained page state, filter-empty selection, drag-event filtering and preview task identity.

Optional screenshots (a new output directory avoids comparing to an older capture):

```sh
ICED_TEST_BACKEND=tiny-skia MEMORIA_UI_CAPTURE_DIR=/tmp/memoria-ui \
  cargo test -p desktop capture_pages -- --ignored --test-threads=1
```

`MEMORIA_TEST_FONT` may point to a locally installed Noto Sans CJK SC font for Linux screenshot inspection. The font is not bundled. Windows continues loading Microsoft YaHei from the system.

Local verification: desktop tests and the mc-core unit/contract tests pass; eight headless screenshots cover all four pages at both sizes. Headless rendering is not a native Windows GPU benchmark, and no FPS/CPU reduction percentage is asserted.

A broader `cargo test --workspace` additionally exercises the unchanged vendored `mc-launcher-core`. Five vendor tests fail here: two network/debug tests panic because reqwest's `rustls-no-provider` has no installed crypto provider, then three tests fail through a poisoned shared cache mutex. The project-owned tests pass; the vendor runtime problem is recorded separately instead of reporting the whole workspace green.

The current workspace layout, per-instance launch buttons, attachment limits and enforced MCP permissions are documented in [Workspace UI](workspace-ui.md).
