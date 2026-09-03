// Typed wrappers over Tauri commands. All PDF work happens in Rust; this is
// the only place the frontend names a command string.
import { invoke } from "@tauri-apps/api/core";

export interface PageInfo {
  index: number;
  width: number;
  height: number;
  rotation: number;
}

export interface AttachmentInfo {
  index: number;
  name: string;
  size: number;
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
  keywords: string | null;
  creator: string | null;
  producer: string | null;
  creation_date: string | null;
  mod_date: string | null;
  file_size: number;
  pdf_version: string;
  encrypted: boolean;
  permissions: number;
  attachments: AttachmentInfo[];
  modified: boolean;
  can_undo: boolean;
  can_redo: boolean;
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

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface SearchHit {
  page_index: number;
  start: number;
  len: number;
  context: string;
  rects: Rect[];
}

export type AnnotKind =
  | "text"
  | "freetext"
  | "line"
  | "square"
  | "circle"
  | "polygon"
  | "polyline"
  | "highlight"
  | "underline"
  | "squiggly"
  | "strikeout"
  | "stamp"
  | "ink"
  | "link"
  | "widget"
  | "popup"
  | "fileattachment"
  | "redact"
  | "other";

export interface Color {
  r: number;
  g: number;
  b: number;
}

export interface Annotation {
  page_index: number;
  index: number;
  kind: AnnotKind;
  rect: Rect;
  contents: string;
  author: string;
  subject: string;
  modified: string;
  color: Color | null;
  interior_color: Color | null;
  border_width: number;
  quads: number[][];
  ink: number[][][];
  hidden: boolean;
  editable: boolean;
}

export interface AnnotationSpec {
  kind: AnnotKind;
  rect: Rect;
  contents?: string;
  author?: string;
  color?: Color | null;
  interior_color?: Color | null;
  border_width?: number;
  quads?: number[][];
  ink?: number[][][];
  font_size?: number;
}

export interface AnnotationPatch {
  rect?: Rect | null;
  contents?: string | null;
  author?: string | null;
  color?: Color | null;
  interior_color?: Color | null;
  border_width?: number | null;
  hidden?: boolean | null;
}

export interface SheafError {
  kind: "engine" | "pdf" | "password_required" | "no_such_document" | "no_such_page" | "io";
  message: string;
}

export interface FormFieldOption {
  label: string;
  selected: boolean;
}

export interface StampSpec {
  /** Pages to stamp; empty = all. */
  pages: number[];
  /** Supports {n}, {total}, {bates}. */
  text: string;
  position:
    | "header-left"
    | "header-center"
    | "header-right"
    | "footer-left"
    | "footer-center"
    | "footer-right"
    | "watermark";
  font_size: number;
  color: Color;
  opacity: number;
  start_at: number;
  bates_digits: number;
}

export interface FormField {
  page_index: number;
  annot_index: number;
  name: string;
  alt_name: string;
  kind: "text" | "checkbox" | "radio" | "combo" | "listbox" | "button" | "signature" | "unknown";
  value: string;
  rect: Rect;
  readonly: boolean;
  required: boolean;
  multiline: boolean;
  password: boolean;
  multiselect: boolean;
  export_value: string;
  checked: boolean;
  options: FormFieldOption[];
}

// ---------- M5: sign and protect ----------

export interface Identity {
  id: string;
  subject: string;
  issuer: string;
  self_signed: boolean;
  not_after: string;
  path: string;
}

export interface SignSpec {
  identity_id: string;
  password: string;
  page: number;
  /** [x1, y1, x2, y2] in PDF points; zero-area for an invisible signature. */
  rect: [number, number, number, number];
  reason?: string | null;
  location?: string | null;
  contact?: string | null;
  name?: string | null;
  lock: boolean;
}

export type SignatureStatus = "valid" | "modified" | "invalid" | "unknown";

export interface SignatureInfo {
  field_name: string;
  signer: string;
  subject: string;
  issuer: string;
  self_signed: boolean;
  signed_at: string | null;
  reason: string | null;
  location: string | null;
  page: number | null;
  rect: [number, number, number, number] | null;
  status: SignatureStatus;
  covers_whole_document: boolean;
  locks_document: boolean;
  messages: string[];
}

export interface SecuritySpec {
  user_password: string;
  owner_password: string;
  allow_print: boolean;
  allow_print_high_quality: boolean;
  allow_modify: boolean;
  allow_copy: boolean;
  allow_annotate: boolean;
  allow_fill_forms: boolean;
  allow_assemble: boolean;
  allow_accessibility: boolean;
}

export function isSheafError(e: unknown): e is SheafError {
  return typeof e === "object" && e !== null && "kind" in e && "message" in e;
}

export function errorMessage(e: unknown): string {
  return isSheafError(e) ? e.message : e instanceof Error ? e.message : String(e);
}

export const api = {
  launchFiles: () => invoke<string[]>("launch_files"),
  openDocument: (path: string, password?: string) =>
    invoke<DocumentInfo>("open_document", { path, password }),
  documentInfo: (id: number) => invoke<DocumentInfo>("document_info", { id }),
  closeDocument: (id: number) => invoke<void>("close_document", { id }),
  renderPage: (id: number, page: number, scale: number, rotation = 0) =>
    invoke<RenderedPage>("render_page", { id, page, scale, rotation }),
  pageText: (id: number, page: number) => invoke<PageText>("page_text", { id, page }),
  outline: (id: number) => invoke<OutlineNode[]>("document_outline", { id }),
  search: (id: number, query: string, caseSensitive = false, wholeWord = false) =>
    invoke<SearchHit[]>("search_document", { id, query, caseSensitive, wholeWord }),
  saveAttachment: (id: number, index: number, path: string) =>
    invoke<void>("save_attachment", { id, index, path }),
  listAnnotations: (id: number, page: number) =>
    invoke<Annotation[]>("list_annotations", { id, page }),
  addAnnotation: (id: number, page: number, spec: AnnotationSpec) =>
    invoke<Annotation>("add_annotation", { id, page, spec }),
  updateAnnotation: (id: number, page: number, index: number, patch: AnnotationPatch) =>
    invoke<Annotation>("update_annotation", { id, page, index, patch }),
  deleteAnnotation: (id: number, page: number, index: number) =>
    invoke<void>("delete_annotation", { id, page, index }),
  undo: (id: number) => invoke<DocumentInfo>("undo", { id }),
  redo: (id: number) => invoke<DocumentInfo>("redo", { id }),
  saveDocument: (id: number, path: string | null, flatten = false) =>
    invoke<DocumentInfo>("save_document", { id, options: { path, flatten } }),
  exportForPrint: (id: number) => invoke<string>("export_for_print", { id }),
  listFormFields: (id: number, page: number) =>
    invoke<FormField[]>("list_form_fields", { id, page }),
  setFormFieldValue: (id: number, page: number, annotIndex: number, value: string) =>
    invoke<FormField>("set_form_field_value", { id, page, annotIndex, value }),
  exportXfdf: (id: number, path: string) => invoke<void>("export_xfdf", { id, path }),
  importXfdf: (id: number, path: string) => invoke<number>("import_xfdf", { id, path }),
  rotatePages: (id: number, pages: number[], delta: number) =>
    invoke<DocumentInfo>("rotate_pages", { id, pages, delta }),
  deletePages: (id: number, pages: number[]) => invoke<DocumentInfo>("delete_pages", { id, pages }),
  movePages: (id: number, pages: number[], dest: number) =>
    invoke<DocumentInfo>("move_pages", { id, pages, dest }),
  insertPages: (id: number, path: string, at: number, password?: string) =>
    invoke<DocumentInfo>("insert_pages", { id, path, at, password }),
  extractPages: (id: number, pages: number[], path: string) =>
    invoke<void>("extract_pages", { id, pages, path }),
  cropPages: (id: number, pages: number[], cropBox: [number, number, number, number]) =>
    invoke<DocumentInfo>("crop_pages", { id, pages, cropBox }),
  stampPages: (id: number, spec: StampSpec) => invoke<DocumentInfo>("stamp_pages", { id, spec }),
  listIdentities: () => invoke<Identity[]>("list_identities"),
  createIdentity: (commonName: string, organization: string | null, password: string) =>
    invoke<Identity>("create_identity", { commonName, organization, password }),
  importIdentity: (path: string, filePassword: string, password: string) =>
    invoke<Identity>("import_identity", { path, filePassword, password }),
  deleteIdentity: (id: string) => invoke<void>("delete_identity", { id }),
  signDocument: (id: number, spec: SignSpec) => invoke<DocumentInfo>("sign_document", { id, spec }),
  listSignatures: (id: number) => invoke<SignatureInfo[]>("list_signatures", { id }),
  protectDocument: (id: number, spec: SecuritySpec) =>
    invoke<DocumentInfo>("protect_document", { id, spec }),
  unprotectDocument: (id: number) => invoke<DocumentInfo>("unprotect_document", { id }),
};
