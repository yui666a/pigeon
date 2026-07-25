import { describe, it, expect } from "vitest";
import {
  computeBounds,
  makeTransform,
  worldToScreen,
  hitTest,
  applyView,
  zoomAt,
  panBy,
  IDENTITY_VIEW,
  MIN_ZOOM,
  MAX_ZOOM,
  type View,
} from "./mapGeometry";
import type { MapPoint } from "../../types/embeddingMap";

const pt = (x: number, y: number, id = "m"): MapPoint => ({
  x, y, mail_id: id, subject: "s", project_id: null, project_name: null, project_color: null,
});

describe("computeBounds", () => {
  it("returns min/max over points", () => {
    const b = computeBounds([pt(-1, 2), pt(3, -4)]);
    expect(b).toEqual({ minX: -1, maxX: 3, minY: -4, maxY: 2 });
  });
});

describe("makeTransform + worldToScreen", () => {
  it("maps world bounds into the padded canvas box", () => {
    const b = { minX: 0, maxX: 10, minY: 0, maxY: 10 };
    const t = makeTransform(b, 100, 100, 10); // padding 10 → 描画領域 80x80
    // 左下(0,0) は左下隅(10, 90) に、右上(10,10) は右上隅(90,10) に。
    expect(worldToScreen(t, 0, 0)).toEqual({ sx: 10, sy: 90 });
    expect(worldToScreen(t, 10, 10)).toEqual({ sx: 90, sy: 10 });
  });
});

describe("hitTest", () => {
  it("returns the nearest point within radius", () => {
    const b = { minX: 0, maxX: 10, minY: 0, maxY: 10 };
    const t = makeTransform(b, 100, 100, 10);
    const points = [pt(0, 0, "a"), pt(10, 10, "b")];
    // 画面座標(10,90) 付近をクリック → a に当たる
    expect(hitTest(points, t, IDENTITY_VIEW, 11, 89, 5)?.mail_id).toBe("a");
  });

  it("returns null when nothing is within radius", () => {
    const b = { minX: 0, maxX: 10, minY: 0, maxY: 10 };
    const t = makeTransform(b, 100, 100, 10);
    expect(hitTest([pt(0, 0)], t, IDENTITY_VIEW, 50, 50, 5)).toBeNull();
  });

  it("returns the nearest point, not the first in range", () => {
    // radius 内に 2 点あり、近い方 "near" を配列の後ろに置く。
    // 「範囲内の最初を返す」実装なら遠い "far" を返してしまい fail する。
    const b = { minX: 0, maxX: 10, minY: 0, maxY: 10 };
    const t = makeTransform(b, 100, 100, 10);
    // world(0,0)→screen(10,90), world(1,1)→screen(18,82)
    const far = pt(0, 0, "far");
    const near = pt(1, 1, "near");
    const points = [far, near]; // 遠い方が先頭
    // click(17,83): near まで≈1.4, far まで≈10.6。両方 radius 15 内。
    expect(hitTest(points, t, IDENTITY_VIEW, 17, 83, 15)?.mail_id).toBe("near");
  });

  it("ズーム後の画面位置で当たる", () => {
    const points = [pt(0, 0, "m1"), pt(10, 10, "m2")];
    const t = makeTransform(computeBounds(points), 800, 800, 40);
    const v: View = { k: 2, ox: -100, oy: -100 };
    const base = worldToScreen(t, 10, 10);
    const s = applyView(v, base.sx, base.sy);
    expect(hitTest(points, t, v, s.sx, s.sy, 6)?.mail_id).toBe("m2");
    // ズーム前の位置ではもう当たらない
    expect(hitTest(points, t, v, base.sx, base.sy, 6)).toBeNull();
  });
});

describe("applyView", () => {
  it("恒等ビューでは座標を変えない", () => {
    expect(applyView(IDENTITY_VIEW, 100, 200)).toEqual({ sx: 100, sy: 200 });
  });

  it("拡大率とオフセットを適用する", () => {
    const v: View = { k: 2, ox: 10, oy: -20 };
    expect(applyView(v, 100, 200)).toEqual({ sx: 210, sy: 380 });
  });
});

describe("zoomAt", () => {
  it("カーソル位置の画面座標が不動点になる", () => {
    // ズーム前にカーソル (300, 400) にあった画面点はズーム後も (300, 400) にある
    const v: View = { k: 2, ox: 50, oy: 60 };
    // カーソルに写っていた「ビュー適用前」の座標を逆算しておく
    const baseX = (300 - v.ox) / v.k;
    const baseY = (400 - v.oy) / v.k;
    const zoomed = zoomAt(v, 300, 400, 1.5);
    expect(zoomed.k).toBeCloseTo(3);
    const after = applyView(zoomed, baseX, baseY);
    expect(after.sx).toBeCloseTo(300);
    expect(after.sy).toBeCloseTo(400);
  });

  it("上限でクランプされる", () => {
    const v: View = { k: MAX_ZOOM, ox: 0, oy: 0 };
    expect(zoomAt(v, 0, 0, 2).k).toBe(MAX_ZOOM);
  });

  it("下限でクランプされ全体表示より引けない", () => {
    const zoomed = zoomAt(IDENTITY_VIEW, 100, 100, 0.5);
    expect(zoomed.k).toBe(MIN_ZOOM);
    // クランプで k が変わらないときはオフセットも動かさない（画面が流れない）
    expect(zoomed).toEqual(IDENTITY_VIEW);
  });
});

describe("panBy", () => {
  it("オフセットだけを移動する", () => {
    const v: View = { k: 3, ox: 10, oy: 20 };
    expect(panBy(v, 5, -7)).toEqual({ k: 3, ox: 15, oy: 13 });
  });
});
