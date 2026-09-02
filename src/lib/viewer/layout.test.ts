import { describe, expect, it } from "vitest";
import {
  clampZoom,
  currentPage,
  fitPageZoom,
  fitWidthZoom,
  layoutContinuous,
  nextZoom,
  totalHeight,
  visiblePages,
} from "./layout";

const a4 = { width: 595, height: 842 };
const pages = [a4, a4, a4, a4];

describe("zoom", () => {
  it("steps up and down through presets", () => {
    expect(nextZoom(1, 1)).toBe(1.25);
    expect(nextZoom(1, -1)).toBe(0.75);
    expect(nextZoom(1.1, 1)).toBe(1.25);
    expect(nextZoom(1.1, -1)).toBe(1);
  });
  it("clamps", () => {
    expect(clampZoom(0)).toBe(0.1);
    expect(clampZoom(100)).toBe(16);
    expect(nextZoom(8, 1)).toBeCloseTo(10);
  });
  it("fit width fills viewport minus gutters", () => {
    const z = fitWidthZoom([a4], 1000);
    expect(z).toBeCloseTo(((1000 - 48) / 595) * 0.75, 5);
  });
  it("fit page respects the tighter axis", () => {
    const z = fitPageZoom([a4], 2000, 600);
    expect(z).toBeCloseTo(((600 - 48) / 842) * 0.75, 5);
  });
});

describe("continuous layout", () => {
  it("stacks pages with gaps", () => {
    const l = layoutContinuous(pages, 1);
    expect(l[0].top).toBe(16);
    expect(l[0].height).toBe(Math.round(842 * (96 / 72)));
    expect(l[1].top).toBe(16 + l[0].height + 16);
    expect(totalHeight(l)).toBe(l[3].top + l[3].height + 16);
  });
  it("reports visible pages with overscan", () => {
    const l = layoutContinuous(pages, 1);
    expect(visiblePages(l, 0, 500, 0)).toEqual([0]);
    expect(visiblePages(l, 0, 500, 1)).toEqual([0, 1]);
    expect(visiblePages(l, l[2].top + 10, 200, 1)).toEqual([1, 2, 3]);
  });
  it("finds current page at viewport midpoint", () => {
    const l = layoutContinuous(pages, 1);
    expect(currentPage(l, 0, 500)).toBe(0);
    expect(currentPage(l, l[2].top, 500)).toBe(2);
    expect(currentPage(l, 1e9, 500)).toBe(3);
  });
});
