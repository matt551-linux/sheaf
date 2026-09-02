// Pure viewport math, kept free of DOM and Tauri so it is unit-testable.

export type FitMode = "custom" | "width" | "page";

export const ZOOM_STEPS = [0.25, 0.33, 0.5, 0.67, 0.75, 1, 1.25, 1.5, 2, 3, 4, 6, 8];
export const MIN_ZOOM = 0.1;
export const MAX_ZOOM = 16;

export function clampZoom(z: number): number {
  return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, z));
}

export function nextZoom(current: number, direction: 1 | -1): number {
  if (direction > 0) {
    const s = ZOOM_STEPS.find((z) => z > current + 1e-6);
    return clampZoom(s ?? current * 1.25);
  }
  const s = [...ZOOM_STEPS].reverse().find((z) => z < current - 1e-6);
  return clampZoom(s ?? current / 1.25);
}

export interface PageSize {
  width: number;
  height: number;
}

/** Zoom so the widest page fills `viewportWidth` CSS px at 96 DPI. */
export function fitWidthZoom(pages: PageSize[], viewportWidth: number, gutter = 24): number {
  const maxW = Math.max(1, ...pages.map((p) => p.width));
  return clampZoom(((viewportWidth - gutter * 2) / maxW) * (72 / 96));
}

/** Zoom so the tallest page fits fully within the viewport. */
export function fitPageZoom(pages: PageSize[], vw: number, vh: number, gutter = 24): number {
  const maxW = Math.max(1, ...pages.map((p) => p.width));
  const maxH = Math.max(1, ...pages.map((p) => p.height));
  const byW = (vw - gutter * 2) / maxW;
  const byH = (vh - gutter * 2) / maxH;
  return clampZoom(Math.min(byW, byH) * (72 / 96));
}

/** CSS pixel size of a page at a given zoom (PDF points at 96 DPI). */
export function cssSize(page: PageSize, zoom: number): PageSize {
  const k = (zoom * 96) / 72;
  return { width: Math.round(page.width * k), height: Math.round(page.height * k) };
}

export interface PageLayout extends PageSize {
  index: number;
  top: number;
}

/** Vertical continuous layout: pages stacked, centered by the caller. */
export function layoutContinuous(pages: PageSize[], zoom: number, gap = 16): PageLayout[] {
  let top = gap;
  return pages.map((p, index) => {
    const s = cssSize(p, zoom);
    const l = { index, top, ...s };
    top += s.height + gap;
    return l;
  });
}

export function totalHeight(layout: PageLayout[], gap = 16): number {
  const last = layout[layout.length - 1];
  return last ? last.top + last.height + gap : 0;
}

/** Indices of pages intersecting [scrollTop, scrollTop + viewportHeight], plus `overscan` pages either side. */
export function visiblePages(
  layout: PageLayout[],
  scrollTop: number,
  viewportHeight: number,
  overscan = 1,
): number[] {
  const bottom = scrollTop + viewportHeight;
  let first = -1;
  let last = -1;
  for (const l of layout) {
    if (l.top + l.height >= scrollTop && l.top <= bottom) {
      if (first < 0) first = l.index;
      last = l.index;
    }
  }
  if (first < 0) return [];
  const lo = Math.max(0, first - overscan);
  const hi = Math.min(layout.length - 1, last + overscan);
  const out: number[] = [];
  for (let i = lo; i <= hi; i++) out.push(i);
  return out;
}

/** The page whose area covers the viewport midpoint; used for "current page". */
export function currentPage(layout: PageLayout[], scrollTop: number, viewportHeight: number): number {
  const mid = scrollTop + viewportHeight / 2;
  for (const l of layout) {
    if (mid >= l.top && mid < l.top + l.height + 16) return l.index;
  }
  return layout.length ? (mid < layout[0].top ? 0 : layout.length - 1) : 0;
}
