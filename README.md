# Sheaf

An open-source, cross-platform PDF reader and editor with the goal of near feature parity
with Adobe Acrobat and a straightforward user experience.

Status: early. Milestones 0 to 7 (reader, annotate and comment, forms, organize pages, sign and protect, edit, redact/compare/OCR/accessibility) are working; CI builds installers for Windows, macOS, and Linux. See the roadmap below.

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
| M3 | Forms: fill (text, checkbox, radio, combo, listbox), required-field validation, XFDF export/import | working |
| M4 | Organize pages: reorder, rotate, delete, insert, extract, crop, headers/footers, Bates, watermark | working |
| M5 | Sign and protect: self-signed or imported (.p12) digital IDs, visible signatures, verification, DocMDP lock, AES-256 password and permissions | working |
| M6 | Edit: edit, move, scale and delete text runs and images, add text and images, links, create PDF from images, export pages to PNG and text | working (Office/HTML import and DOCX/PDF-A export deferred) |
| M7 | Redact (true content removal, by area or by search), Compare (word diff plus visual diff), OCR (local, ocrs models downloaded on demand), Accessibility checks | working |
| M8 | Installers for all platforms and auto-update: working. Localization and plugin API: planned |

## Installing

Every release on the [Releases page](https://github.com/matt551-linux/sheaf/releases) ships installers built by CI:

| Platform | File | Notes |
| --- | --- | --- |
| Windows x64 / ARM64 | `Sheaf_<ver>_x64-setup.exe`, `..._arm64-setup.exe` (also `.msi`) | Not code-signed yet; SmartScreen shows "More info > Run anyway" |
| macOS (Intel and Apple Silicon) | `Sheaf_<ver>_universal.dmg` | Not notarized yet; right-click > Open on first launch |
| Debian, Ubuntu, Mint, Pop!_OS | `Sheaf_<ver>_amd64.deb`, `..._arm64.deb` | `sudo apt install ./Sheaf_*.deb` pulls WebKitGTK 4.1 and GTK 3 |
| Fedora, RHEL, Rocky, Alma, openSUSE | `Sheaf-<ver>-1.x86_64.rpm`, `...aarch64.rpm` | `sudo dnf install ./Sheaf-*.rpm` |
| Arch, Omarchy, Manjaro, NixOS, anything else | `Sheaf_<ver>_amd64.AppImage`, `..._aarch64.AppImage` | `chmod +x` and run; needs `webkit2gtk-4.1` installed (Arch: `pacman -S webkit2gtk-4.1`) |

The Linux packages are built on Ubuntu runners but only depend on `libwebkit2gtk-4.1` and `libgtk-3`, which every current distribution ships. An AUR package is planned.

Installed copies check for updates a few seconds after launch and offer a one-click "Update and restart"; File > Check for updates does it on demand. Updates are verified against a signing key embedded in the app.

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
  drives the annotation tools through real DOM events for on-screen verification. `--screenshot "<js>" [out.png]`
  captures the webview, optionally running an expression first.
- Print rasterizes pages with the engine (~192 DPI, annotations included) and opens the system
  print dialog from a hidden iframe. Vector print output is a later milestone.
- Form filling goes through PDFium's form-fill environment (`FORM_*` APIs) so appearance streams
  regenerate and radio groups stay consistent. `scripts/make-form-fixture.py` (reportlab)
  regenerates `tests/fixtures/form.pdf`.
- The form editor (creating new fields) is deferred to a later milestone; M3 covers fill,
  validate, and XFDF interchange.
