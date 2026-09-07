// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import PageCanvas from "./PageCanvas.svelte";
import NavPanel from "./NavPanel.svelte";
import { docStore } from "$lib/stores/document.svelte";
import { api, type RenderedPage, type DocumentInfo } from "$lib/api";

vi.mock("@tauri-apps/plugin-store", () => ({ LazyStore: class {} }));
vi.mock("$lib/api", async (original) => ({
  ...await original<typeof import("$lib/api")>(),
  api: {
    renderPage: vi.fn(async () => ({ index: 0, png_base64: "AA==", width_px: 1, height_px: 1 })),
    pageText: vi.fn(async () => ({ chars: [], text: "" })),
    listAnnotations: vi.fn(async () => []),
    listFormFields: vi.fn(async () => []),
    closeDocument: vi.fn(async () => {}),
  },
}));

let component: ReturnType<typeof mount> | undefined;
let target: HTMLDivElement;
const props = { onOpenNote: () => {}, onGoToPage: () => {} };
// Small metadata only. No PDF engine or large bitmap allocations in this repro.
function documentInfo(id = 1): DocumentInfo {
  return { id, page_count: 100, pages: Array.from({ length: 100 }, (_, index) =>
    ({ index, width: 612, height: 792, rotation: 0 })), attachments: [] } as unknown as DocumentInfo;
}
async function settle() {
  for (let i = 0; i < 12; i++) { await Promise.resolve(); flushSync(); }
}
beforeEach(async () => {
  await docStore.close();
  vi.resetAllMocks();
  docStore.doc = documentInfo();
  docStore.fitMode = "custom";
  docStore.zoom = 1;
  docStore.setViewport(800, 600);
  docStore.navPanel = "thumbnails";
  target = document.createElement("div");
  document.body.append(target);
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(800);
  vi.stubGlobal("ResizeObserver", class { observe() {} unobserve() {} disconnect() {} });
});
afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  target.remove();
  vi.unstubAllGlobals();
});

test("thumbnails render only a bounded window and stop while the panel is hidden", async () => {
  component = mount(NavPanel, { target, props });
  for (let i = 0; i < 20; i++) await settle();
  expect(vi.mocked(api.renderPage).mock.calls.length).toBeGreaterThan(0);
  expect(vi.mocked(api.renderPage).mock.calls.length).toBeLessThanOrEqual(6);
  expect(target.querySelectorAll("aside img").length).toBeLessThanOrEqual(6);
  docStore.navPanel = "none";
  await settle();
  const calls = vi.mocked(api.renderPage).mock.calls.length;
  await settle();
  expect(api.renderPage).toHaveBeenCalledTimes(calls);
});

test("scrolling keeps page views and rendered images bounded", async () => {
  component = mount(PageCanvas, { target, props });
  await settle();
  const scroller = target.querySelector('[role="document"]') as HTMLDivElement;
  for (const page of [10, 30, 60, 90, 0]) {
    scroller.scrollTop = docStore.layout[page].top;
    scroller.dispatchEvent(new Event("scroll"));
    await settle();
    expect(target.querySelector(`[data-page="${page}"]`)).not.toBeNull();
    expect(target.querySelectorAll("[data-page]").length).toBeLessThanOrEqual(4);
    expect(target.querySelectorAll("[data-page] img").length).toBeLessThanOrEqual(4);
  }
});

test("a new document resets the virtual page viewport", async () => {
  component = mount(PageCanvas, { target, props });
  await settle();
  const scroller = target.querySelector('[role="document"]') as HTMLDivElement;
  scroller.scrollTop = docStore.layout[90].top;
  scroller.dispatchEvent(new Event("scroll"));
  await settle();
  const next = documentInfo(2);
  next.pages = next.pages.slice(0, 2);
  next.page_count = 2;
  docStore.doc = next;
  await settle();
  expect(target.querySelector('[data-page="0"]')).not.toBeNull();
  expect(scroller.scrollTop).toBe(0);
});

test("switching a scrolled thumbnail panel to a short document starts at page one", async () => {
  component = mount(NavPanel, { target, props });
  await settle();
  const scroller = target.querySelector("aside .overflow-auto") as HTMLDivElement;
  scroller.scrollTop = 224 * 80;
  scroller.dispatchEvent(new Event("scroll"));
  await settle();
  expect(target.querySelector('img[alt="Page 81"]')).not.toBeNull();
  const next = documentInfo(2);
  next.pages = next.pages.slice(0, 2);
  next.page_count = 2;
  docStore.doc = next;
  await settle();
  expect(target.querySelector('img[alt="Page 1"]')).not.toBeNull();
});

test("render cache is bounded by estimated image bytes, not only page count", async () => {
  // Metadata simulates two decoded 36 MB pages; payloads remain four bytes.
  vi.mocked(api.renderPage).mockResolvedValue({ index: 0, width_px: 3000, height_px: 3000, png_base64: "AA==" });
  await docStore.pageImage(0);
  await docStore.pageImage(1);
  await docStore.pageImage(0);
  expect(api.renderPage).toHaveBeenCalledTimes(3);
});

test("closing a document prevents pending renders from repopulating its cache", async () => {
  let finish!: (value: RenderedPage) => void;
  vi.mocked(api.renderPage).mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
  const pending = docStore.pageImage(0);
  await docStore.close();
  finish({ index: 0, width_px: 1, height_px: 1, png_base64: "AA==" });
  await pending;
  docStore.doc = documentInfo();
  await docStore.pageImage(0);
  expect(api.renderPage).toHaveBeenCalledTimes(2);
});

test.each(["ensureText", "ensureAnnots", "ensureFormFields"] as const)("%s coalesces concurrent requests and ignores a closed session", async (method) => {
  const apiMethod = { ensureText: "pageText", ensureAnnots: "listAnnotations", ensureFormFields: "listFormFields" } as const;
  let finish!: (value: any) => void;
  const request = vi.mocked(api[apiMethod[method]]);
  request.mockImplementation(() => new Promise((resolve) => { finish = resolve; }) as any);
  const first = docStore[method](0);
  const second = docStore[method](0);
  expect(request).toHaveBeenCalledTimes(1);
  await docStore.close();
  docStore.doc = documentInfo(2);
  finish(method === "ensureText" ? { chars: [], text: "old" } : []);
  await Promise.all([first, second]);
  expect(Object.keys(docStore.texts)).toHaveLength(0);
  expect(Object.keys(docStore.annots)).toHaveLength(0);
  expect(Object.keys(docStore.formFields)).toHaveLength(0);
});

test("changing documents releases old page images before new renders complete", async () => {
  component = mount(PageCanvas, { target, props });
  await settle();
  expect(target.querySelectorAll("[data-page] img").length).toBeGreaterThan(0);
  vi.mocked(api.renderPage).mockImplementationOnce(() => new Promise(() => {}));
  docStore.doc = documentInfo(2);
  flushSync();
  expect(target.querySelectorAll("[data-page] img")).toHaveLength(0);
});

test("opening a long document loads overlays only for the viewport, then settles", async () => {
  component = mount(PageCanvas, { target, props });
  await settle();
  expect(vi.mocked(api.pageText).mock.calls.length).toBeGreaterThan(0);
  expect(vi.mocked(api.pageText).mock.calls.length).toBeLessThanOrEqual(3);
  expect(vi.mocked(api.listAnnotations).mock.calls.length).toBeLessThanOrEqual(3);
  expect(vi.mocked(api.listFormFields).mock.calls.length).toBeLessThanOrEqual(3);
  const calls = vi.mocked(api.renderPage).mock.calls.length;
  await settle();
  expect(api.renderPage).toHaveBeenCalledTimes(calls);
});
