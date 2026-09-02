# Sheaf

An open-source, cross-platform PDF reader and editor with the goal of near feature parity
with Adobe Acrobat and a straightforward user experience.

Status: early. Milestones 0 to 2 (reader, annotate and comment) are working on Windows ARM64; see the roadmap below.

## Stack

- Tauri 2 shell, Rust core, PDFium rendering engine (via `pdfium-render`)
- Svelte 5 + TypeScript + Tailwind 4 frontend
- Apache-2.0 license; PDFium is BSD-3

## Why not LibreOffice as a backend

LibreOffice Draw imports PDFs as vector shapes and loses text flow, forms, annotations and structure.
PDFium (the engine inside Chrome and Edge) gives rendering, forms, annotations, text extraction, page
operations and text object editing natively. Where PDFium's public API is thin (redaction, digital
signatures, comparison) Sheaf implements the feature in Rust directly on the PDF object model.

## Developing

Prerequisites: Rust stable, Node 22+, pnpm, and the Tauri 2 platform prerequisites for your OS
(https://tauri.app/start/prerequisites/).

```bash
pnpm install
node scripts/fetch-pdfium.mjs   # downloads the PDFium binary for your platform
pnpm tauri dev
```

Windows-on-ARM in git-bash: `source scripts/dev-env-windows.sh` first so cargo finds MSVC `link.exe`
and the Windows SDK `rc.exe` (git-bash otherwise shadows `link` with the coreutils command).

Tests:

```bash
pnpm test                                  # frontend (vitest)
cd src-tauri && cargo test                 # Rust engine against tests/fixtures/*.pdf
pnpm check                                 # svelte-check / TypeScript
```

## Roadmap

| Milestone | Scope | Status |
|---|---|---|
| M0 | Open, render, continuous scroll, zoom, fit modes, rotate view, thumbnails, bookmarks, find, password prompt, drag and drop | working |
| M1 | Text selection and copy, search hit highlighting, single/continuous/two-up, dark UI, night mode, recent files, properties, attachments, file association, print handoff | working |
| M2 | Annotate and comment: highlight, underline, strikethrough, squiggly, sticky note, text box, pen, rectangle, ellipse, eraser, move, comments panel, note editor, undo/redo, save, save as, flatten | working |
| M3 | Forms: fill, validate, prepare form editor, FDF/XFDF | planned |
| M4 | Organize pages: reorder, rotate, delete, insert, split, merge, crop, headers/footers, Bates, watermark | planned |
| M5 | Sign and protect: Fill and Sign, digital signatures (PAdES), password and permissions | planned |
| M6 | Edit: text and image editing, links, create PDF from images/Office/HTML, export to images/text/DOCX/PDF-A | planned |
| M7 | Redact, Compare, OCR, Accessibility tools | planned |
| M8 | Installers for all platforms, auto-update, localization, plugin API | planned |

Platform targets: Windows (x64, ARM64), macOS (universal), Linux (deb, rpm, AppImage, AUR).

## Keyboard shortcuts (so far)

Ctrl+O open, Ctrl+S save, Ctrl+Shift+S save as, Ctrl+P print, Ctrl+D properties, Ctrl+F find, F3 / Shift+F3 next
and previous hit, Ctrl+C copy selected text, Ctrl+Z / Ctrl+Y undo and redo, Ctrl+W close, Ctrl+plus/minus zoom,
Ctrl+0 fit page, Ctrl+1 actual size, Ctrl+2 fit width, Ctrl+Shift+plus/minus rotate view, Ctrl+Shift+N go to page,
PageUp/PageDown, Home/End, Delete removes the selected annotation, Escape returns to the Select tool.

Tool keys: V select, H hand, U highlight, N note, T text box, P pen, R rectangle, E ellipse.

## Developer notes

- `scripts/cdp.py` evaluates JavaScript inside the running app when it is launched with
  `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223` (Windows). `scripts/ui-scenario.js`
  drives the annotation tools through real DOM events for on-screen verification.
- Print currently exports the current state to a temp PDF and opens it with the system handler.
  A native print dialog is planned.
