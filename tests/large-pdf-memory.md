# Large-document memory regression

## Safe automated reproduction

```sh
pnpm install --frozen-lockfile
pnpm exec vitest run src/lib/components/viewer-memory.test.ts
pnpm test
pnpm check
pnpm build
cargo test --manifest-path src-tauri/Cargo.toml --locked -j 2
```

The component tests run the real Svelte client components, effects and document
store in happy-dom, replacing only Tauri IPC with small deterministic responses.
They use 100 page-metadata records and four-character image payloads, not a large
PDF or actual large decoded bitmaps. Layout measurements are fixed at 800x600.
No AppImage or uncontrolled memory stress is needed.

## Failures observed before the corresponding fixes

- Startup requested **100 text layers**, against an expected viewport maximum of
  three. `PageCanvas` mounted every `PageView`; each view immediately fetched text,
  annotations and form fields, despite the "when visible" comment.
- Thumbnails issued **87 render calls** in the bounded microtask test window,
  against a maximum of six. The loop was still traversing all 100 pages.
- Switching documents retained **two old page images** until replacement renders
  completed, rather than releasing them immediately.
- Concurrent text, annotation and form requests each issued **two IPC calls**
  rather than sharing one pending operation.
- Two simulated 3000x3000 renders both remained cached under the old 40-page cap;
  requesting the first again caused only two total render calls, rather than
  three after byte-budget eviction.
- A render completing after close repopulated the old cache; reopening the same
  session identifier reused it (one IPC call rather than two).

## Fix boundaries

- Only viewport pages plus overscan mount interactive page views. The full layout
  still supplies scroll geometry, including continuous, single and two-up modes.
- Thumbnail rows are virtualized with fixed 224px spacing and one-row overscan.
  Leaving the panel or changing documents cancels further scheduling and ignores
  the outstanding result. Switching documents resets both virtual scroll windows.
- Page-image component state retains only its current window. Effect cleanup
  rejects late renders, including close and unmount.
- The render cache has a 64 MiB estimated-byte limit as well as its existing
  40-entry limit. Cost includes RGBA dimensions plus a conservative two-byte JS
  string estimate; this is **not a bound on total process RSS**.
- Lazy page-data requests coalesce, update per-page properties instead of copying
  whole maps, and cannot publish into a replaced cache or document session.

The suspected endless image-map effect loop was not reproduced: image writes
happen in promise continuations outside synchronous dependency tracking. Tests
also check that stationary rendering settles. Do not move image-map reads into
the synchronous render effect without explicitly excluding dependency tracking.

## Verification on this checkout

- `pnpm test`: 29 passed across 3 files (11 new memory-regression tests).
- `pnpm check`: 0 errors, 0 warnings.
- `pnpm build`: succeeds; static site written to `build/`. Vite reports the
  existing mixed static/dynamic import of the dialog plugin, not a build error.
- `git diff --check`: passes.

## Remaining validation / limitations

No user failing PDF was supplied. These tests demonstrate specific excessive-work
and retention defects, not proof that every WebKit abort or platform-specific
crash has been eliminated. happy-dom does not measure WebKit decoded-image RSS.

Backend code is unchanged: document-info still loads each page for rotation;
rendering still has a scale cap but no absolute pixel-allocation budget; command
responses still use the existing channel mechanism. An unusually large page or
extreme zoom can therefore still demand a large individual allocation. Print and
explicit whole-document tools also still perform whole-document work. Lazy text
and annotation caches retain pages visited during the current document session.

Before shipping, repeat with the user's actual failing PDF in Linux WebKit and
Windows WebView2, recording process RSS while opening, scrolling, zooming,
switching thumbnail tabs and closing. Do not characterize this frontend patch as
a verified fix for an unavailable binary/PDF combination.
