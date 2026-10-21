# iced UI migration and responsiveness

Based on upstream `9cd898865c32e636801fe888b6ccde8ae394eb22` and the four supplied design references. Slint source and artwork remain as migration references; the desktop binary uses iced only.

## Implemented

- Four persistent page states behind the fixed sidebar. Navigating clears modal/drag/layout-edit state without discarding the download filters, selected version or AI input.
- Downloads: five resource categories, search, release/snapshot and loader filters, date ordering, selection, four detail tabs, banner controls and loading/error/empty states. Catalogue loading uses the existing asynchronous `CatalogProvider` contract, once on first entry (or explicitly on refresh).
- AI: one prompt field, target version/language, web-search option, two batches of eight suggestions, four examples, translation entry points and an independently scrollable task panel. Small windows use two-column cards and wrap the composer controls.
- Settings: seven navigable tabs, five general-settings cards, keyword search and an explicit explanation for categories whose configuration service is not connected.
- Retained the existing home layout editor, instance selection/reordering, featured cards, queue controls and account preview.

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

The existing core provides `MockCatalogProvider`, sample instances and in-memory sample tasks. They are visibly labelled as previews. Mod/resource/shader catalogues, actual installation, attachment import, AI execution, configuration persistence and non-general settings were not implemented by the original desktop interface. This migration does not pretend to execute them: install/submit/import actions explain the missing connection and never fabricate successful work or send data.

No `mc-core` runtime, authentication, process or scanning contracts were changed.

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
