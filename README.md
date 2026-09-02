# Sheaf

An open-source, cross-platform PDF reader and editor with the goal of near feature parity
with Adobe Acrobat and a straightforward user experience.

Status: early. Milestone 0 (reader core) is working on Windows ARM64; see the roadmap below.

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
| M1 | Text selection and copy, search highlighting, view modes, print, recent files, properties, attachments | next |
| M2 | Annotate and comment: highlight, notes, ink, shapes, stamps, comments panel, undo/redo, incremental save | planned |
| M3 | Forms: fill, validate, prepare form editor, FDF/XFDF | planned |
| M4 | Organize pages: reorder, rotate, delete, insert, split, merge, crop, headers/footers, Bates, watermark | planned |
| M5 | Sign and protect: Fill and Sign, digital signatures (PAdES), password and permissions | planned |
| M6 | Edit: text and image editing, links, create PDF from images/Office/HTML, export to images/text/DOCX/PDF-A | planned |
| M7 | Redact, Compare, OCR, Accessibility tools | planned |
| M8 | Installers for all platforms, auto-update, localization, plugin API | planned |

Platform targets: Windows (x64, ARM64), macOS (universal), Linux (deb, rpm, AppImage, AUR).

## Keyboard shortcuts (so far)

Ctrl+O open, Ctrl+F find, Ctrl+W close, Ctrl+plus/minus zoom, Ctrl+0 fit page, Ctrl+1 actual size,
Ctrl+2 fit width, Ctrl+Shift+plus/minus rotate view, Ctrl+Shift+N go to page, PageUp/PageDown, Home/End.
