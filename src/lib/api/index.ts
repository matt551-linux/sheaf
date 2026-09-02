// Typed wrappers over Tauri commands. All PDF work happens in Rust; this is
// the only place the frontend names a command string.
import { invoke } from "@tauri-apps/api/core";

export interface PageInfo {
  index: number;
  width: number;
  height: number;
  rotation: number;
}

export interface DocumentInfo {
  id: number;
  path: string;
  file_name: string;
  page_count: number;
  pages: PageInfo[];
  title: string | null;
  author: string | null;
  subject: string | null;
  creator: string | null;
  producer: string | null;
}

export interface RenderedPage {
  index: number;
  width_px: number;
  height_px: number;
  png_base64: string;
}

export interface TextChar {
  ch: string;
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface PageText {
  index: number;
  text: string;
  chars: TextChar[];
}

export interface OutlineNode {
  title: string;
  page_index: number | null;
  children: OutlineNode[];
}

export interface SearchHit {
  page_index: number;
  start: number;
  len: number;
  context: string;
}

export interface SheafError {
  kind:
    | "engine"
    | "pdf"
    | "password_required"
    | "no_such_document"
    | "no_such_page"
    | "io";
  message: string;
}

export function isSheafError(e: unknown): e is SheafError {
  return typeof e === "object" && e !== null && "kind" in e && "message" in e;
}

export const api = {
  openDocument: (path: string, password?: string) =>
    invoke<DocumentInfo>("open_document", { path, password }),
  closeDocument: (id: number) => invoke<void>("close_document", { id }),
  renderPage: (id: number, page: number, scale: number, rotation = 0) =>
    invoke<RenderedPage>("render_page", { id, page, scale, rotation }),
  pageText: (id: number, page: number) => invoke<PageText>("page_text", { id, page }),
  outline: (id: number) => invoke<OutlineNode[]>("document_outline", { id }),
  search: (id: number, query: string, caseSensitive = false) =>
    invoke<SearchHit[]>("search_document", { id, query, caseSensitive }),
};
