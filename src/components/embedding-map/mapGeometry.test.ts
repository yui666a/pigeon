import { describe, it, expect } from "vitest";
import { computeBounds, makeTransform, worldToScreen, hitTest } from "./mapGeometry";
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
    expect(hitTest(points, t, 11, 89, 5)?.mail_id).toBe("a");
  });

  it("returns null when nothing is within radius", () => {
    const b = { minX: 0, maxX: 10, minY: 0, maxY: 10 };
    const t = makeTransform(b, 100, 100, 10);
    expect(hitTest([pt(0, 0)], t, 50, 50, 5)).toBeNull();
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
    expect(hitTest(points, t, 17, 83, 15)?.mail_id).toBe("near");
  });
});
