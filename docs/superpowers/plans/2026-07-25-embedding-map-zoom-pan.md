# 埋め込みマップ ズーム / パン / ホバー 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 埋め込みマップの Canvas にホイールズーム・ドラッグパン・点ホバーのツールチップを追加し、密集部の点を判別・操作できるようにする。

**Architecture:** Phase A の基底変換（`makeTransform` = 全体を 800×800 に収める）はそのまま残し、その**上に載る画面座標のビュー変換 `View { k, ox, oy }`**（拡大率 + オフセット）を導入する。ズーム・パン・ヒットテストは `mapGeometry.ts` の純関数として TDD し、`EmbeddingMapCanvas` はそれを繋ぐだけにする。空白部の mousedown = パン、点の上の mousedown = 既存の D&D（Phase B）で、既存 Props（`onPointMouseDown`）は変えない。設計書 `2026-07-22-embedding-map-window-design.md` §4.6・§5・§10 に対応（Phase A/B で意図的に見送った分）。

**Tech Stack:** React 19 + TypeScript、素の Canvas 2D、Vitest。Rust 側の変更なし。

## Global Constraints

- TDD 必須（Red → Green → Refactor）。純関数（mapGeometry）を先にテストで固める
- TypeScript: `any` 禁止。1ファイル1コンポーネント
- コミットは Conventional Commits 形式、1コミット = 1意図
- 可視化ライブラリは導入しない（設計書 §4.6。素の Canvas のまま）
- ズーム下限は 1（全体表示より引かない）、上限は 40。ダブルクリックで全体表示へリセット
- 点の描画半径・ヒット半径は**ズームしても画面上のピクセルで一定**（スケールしない）
- フロントのテスト収集範囲は `src/**/*.{test,spec}.{ts,tsx}`。実行は `npx vitest run <file>`（全体は `pnpm test`）

## 前提（現状）

- `src/components/embedding-map/mapGeometry.ts`: `Bounds` / `Transform` / `computeBounds` / `makeTransform` / `worldToScreen` / `hitTest(points, t, sx, sy, radius)`
- `src/components/embedding-map/EmbeddingMapCanvas.tsx`: 800×800 固定 `<canvas>`。`onMouseDown` で `hitTest` → 当たれば `onPointMouseDown(point, e)`（Phase B の D&D 起点）。描画は `useEffect([points])`
- `src/components/embedding-map/mapGeometry.test.ts`: `hitTest` 等の既存テストあり（`hitTest` の呼び出しシグネチャ変更に追従させる）
- React の `onWheel` はルートに passive で付くため `preventDefault()` が効かない → **native の `addEventListener("wheel", handler, { passive: false })`** を使う

---

### Task 1: mapGeometry — ビュー変換の純関数

**Files:**
- Modify: `src/components/embedding-map/mapGeometry.ts`
- Modify: `src/components/embedding-map/mapGeometry.test.ts`

**Interfaces:**
- Produces:
  - `interface View { k: number; ox: number; oy: number }`（画面座標に対する拡大率とオフセット）
  - `const IDENTITY_VIEW: View`（`{ k: 1, ox: 0, oy: 0 }`）
  - `const MIN_ZOOM = 1` / `const MAX_ZOOM = 40`（export する。Canvas 側と共有）
  - `applyView(v: View, sx: number, sy: number): { sx: number; sy: number }`
  - `zoomAt(v: View, cx: number, cy: number, factor: number): View`（カーソル位置 `(cx, cy)` を不動点にズーム。`k` は `[MIN_ZOOM, MAX_ZOOM]` にクランプ）
  - `panBy(v: View, dx: number, dy: number): View`
  - **シグネチャ変更**: `hitTest(points, t, view, sx, sy, radius)`（`view` 引数を追加。Task 2 が使う）

- [ ] **Step 1: 失敗するテストを書く**

`mapGeometry.test.ts` に追記（既存 `hitTest` テストの呼び出しには `IDENTITY_VIEW` を挿入して追従させる）:

```ts
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

describe("hitTest（ビュー付き）", () => {
  it("ズーム後の画面位置で当たる", () => {
    const points = [point("m1", 0, 0), point("m2", 10, 10)];
    const t = makeTransform(computeBounds(points), 800, 800, 40);
    const v: View = { k: 2, ox: -100, oy: -100 };
    const base = worldToScreen(t, 10, 10);
    const s = applyView(v, base.sx, base.sy);
    expect(hitTest(points, t, v, s.sx, s.sy, 6)?.mail_id).toBe("m2");
    // ズーム前の位置ではもう当たらない
    expect(hitTest(points, t, v, base.sx, base.sy, 6)).toBeNull();
  });
});
```

既存テスト側の注意: `point(...)` ヘルパは既存の `mapGeometry.test.ts` にあるものを使う（無ければ `MapPoint` を組み立てる小ヘルパを追加）。既存の `hitTest(points, t, sx, sy, r)` 呼び出しはすべて `hitTest(points, t, IDENTITY_VIEW, sx, sy, r)` に書き換える。

- [ ] **Step 2: テストが失敗することを確認**

Run: `npx vitest run src/components/embedding-map/mapGeometry.test.ts`
Expected: FAIL（`applyView` 等が未定義）

- [ ] **Step 3: 実装**

`mapGeometry.ts` に追加・変更:

```ts
/** 基底変換（全体を canvas に収める）の上に載る、画面座標のビュー変換。 */
export interface View {
  /** 拡大率。1 = 全体表示 */
  k: number;
  ox: number;
  oy: number;
}

export const IDENTITY_VIEW: View = { k: 1, ox: 0, oy: 0 };
export const MIN_ZOOM = 1;
export const MAX_ZOOM = 40;

export function applyView(v: View, sx: number, sy: number): { sx: number; sy: number } {
  return { sx: sx * v.k + v.ox, sy: sy * v.k + v.oy };
}

/**
 * カーソル位置 (cx, cy) を不動点にしてズームする。
 * k がクランプで変わらない場合はオフセットも動かさない。
 */
export function zoomAt(v: View, cx: number, cy: number, factor: number): View {
  const k = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, v.k * factor));
  if (k === v.k) return v;
  const ratio = k / v.k;
  return {
    k,
    ox: cx - (cx - v.ox) * ratio,
    oy: cy - (cy - v.oy) * ratio,
  };
}

export function panBy(v: View, dx: number, dy: number): View {
  return { k: v.k, ox: v.ox + dx, oy: v.oy + dy };
}
```

`hitTest` の変更（`view` を通して画面位置を比較する）:

```ts
export function hitTest(
  points: MapPoint[],
  t: Transform,
  view: View,
  sx: number,
  sy: number,
  radius: number,
): MapPoint | null {
  let best: MapPoint | null = null;
  let bestDist = radius * radius;
  for (const p of points) {
    const base = worldToScreen(t, p.x, p.y);
    const s = applyView(view, base.sx, base.sy);
    const dx = s.sx - sx;
    const dy = s.sy - sy;
    const d = dx * dx + dy * dy;
    if (d <= bestDist) {
      bestDist = d;
      best = p;
    }
  }
  return best;
}
```

- [ ] **Step 4: テストが通ることを確認**

Run: `npx vitest run src/components/embedding-map/mapGeometry.test.ts`
Expected: PASS（既存分 + 新規分すべて）

注意: この時点で `EmbeddingMapCanvas.tsx` が `hitTest` の旧シグネチャを呼んでいて `pnpm build` は落ちる。Task 2 で直すので、このタスクではテストのみ通ればよい（コミットは Task 2 とまとめず、ここで行う — ビルド断面が壊れるのを避けたい場合は Task 2 の Step 1 だけ先に済ませてから両方まとめてコミットしてもよいが、既定は分割コミット）。

- [ ] **Step 5: コミット**

```bash
git add src/components/embedding-map/mapGeometry.ts src/components/embedding-map/mapGeometry.test.ts
git commit -m "feat(ui): マップのビュー変換（ズーム・パン・ヒットテスト）を追加"
```

---

### Task 2: EmbeddingMapCanvas — ホイールズーム・パン・ホバーの統合

**Files:**
- Modify: `src/components/embedding-map/EmbeddingMapCanvas.tsx`

**Interfaces:**
- Consumes: Task 1 の `View` / `IDENTITY_VIEW` / `applyView` / `zoomAt` / `panBy` / 新 `hitTest`
- Produces: Props は変更なし（`points`, `onPointMouseDown`）。`VisualizationRoot` の変更は不要

挙動:
- **ホイール**: カーソル位置を不動点にズーム（1 ノッチ = ×1.2 / ÷1.2）。native listener（`{ passive: false }`）+ `preventDefault()` でページスクロールを抑止
- **パン**: 点に当たらない場所で mousedown → window の mousemove で `panBy`、mouseup で終了
- **点の D&D**: 従来どおり。当たり判定だけ view 対応の `hitTest` に変わる
- **ホバー**: パン中でなければ mousemove で `hitTest` し、当たった点の件名をツールチップ表示。カーソルは点の上で `pointer`
- **ダブルクリック**: `IDENTITY_VIEW` にリセット
- 描画は `applyView(view, worldToScreen(t, ...))`。点の半径は固定（2 / 3.5px のまま）

マウスイベントの合成と Canvas 描画（jsdom に 2D context が無い）に跨るため、このタスクの単体テストは書かない。ロジックは Task 1 の純関数で担保済み。動作は Task 3 の実機確認で検証する。

- [ ] **Step 1: 実装**

`EmbeddingMapCanvas.tsx` 全体を以下へ差し替え:

```tsx
import { useEffect, useRef, useState } from "react";
import type { MapPoint } from "../../types/embeddingMap";
import {
  computeBounds,
  makeTransform,
  worldToScreen,
  applyView,
  zoomAt,
  panBy,
  hitTest,
  IDENTITY_VIEW,
  type Transform,
  type View,
} from "./mapGeometry";

const PADDING = 40;
const UNASSIGNED_COLOR = "#cccccc";
const DEFAULT_PROJECT_COLOR = "#6b7280";
const HIT_RADIUS = 6;
const WHEEL_FACTOR = 1.2;

interface Props {
  points: MapPoint[];
  /** 点の上で mousedown したとき（クリックかドラッグかは呼び出し側が判定） */
  onPointMouseDown: (point: MapPoint, e: React.MouseEvent) => void;
}

interface Hover {
  sx: number;
  sy: number;
  subject: string;
}

export function EmbeddingMapCanvas({ points, onPointMouseDown }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const transformRef = useRef<Transform | null>(null);
  const viewRef = useRef<View>(IDENTITY_VIEW);
  const panningRef = useRef(false);
  const [view, setView] = useState<View>(IDENTITY_VIEW);
  const [hover, setHover] = useState<Hover | null>(null);

  viewRef.current = view;

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const width = canvas.width;
    const height = canvas.height;
    const bounds = computeBounds(points);
    const t = makeTransform(bounds, width, height, PADDING);
    transformRef.current = t;

    ctx.clearRect(0, 0, width, height);

    const screenOf = (p: MapPoint) => {
      const base = worldToScreen(t, p.x, p.y);
      return applyView(view, base.sx, base.sy);
    };

    // 未分類を先に背面へ（薄いグレー・小さめ）。案件を覆い隠さないため。
    for (const p of points) {
      if (p.project_id) continue;
      const s = screenOf(p);
      ctx.fillStyle = UNASSIGNED_COLOR;
      ctx.globalAlpha = 0.4;
      ctx.beginPath();
      ctx.arc(s.sx, s.sy, 2, 0, Math.PI * 2);
      ctx.fill();
    }
    // 案件を前面へ（色付き・大きめ）
    for (const p of points) {
      if (!p.project_id) continue;
      const s = screenOf(p);
      ctx.fillStyle = p.project_color ?? DEFAULT_PROJECT_COLOR;
      ctx.globalAlpha = 0.85;
      ctx.beginPath();
      ctx.arc(s.sx, s.sy, 3.5, 0, Math.PI * 2);
      ctx.fill();
    }
    ctx.globalAlpha = 1;
  }, [points, view]);

  // React の onWheel はルートに passive で付き preventDefault が効かないため
  // native listener を使う（ズーム時にページスクロールさせない）
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const handleWheel = (e: WheelEvent) => {
      e.preventDefault();
      const rect = canvas.getBoundingClientRect();
      const cx = e.clientX - rect.left;
      const cy = e.clientY - rect.top;
      const factor = e.deltaY < 0 ? WHEEL_FACTOR : 1 / WHEEL_FACTOR;
      setView((v) => zoomAt(v, cx, cy, factor));
    };
    canvas.addEventListener("wheel", handleWheel, { passive: false });
    return () => canvas.removeEventListener("wheel", handleWheel);
  }, []);

  const canvasPos = (e: React.MouseEvent): { sx: number; sy: number } | null => {
    const canvas = canvasRef.current;
    if (!canvas) return null;
    const rect = canvas.getBoundingClientRect();
    return { sx: e.clientX - rect.left, sy: e.clientY - rect.top };
  };

  const handleMouseDown = (e: React.MouseEvent<HTMLCanvasElement>) => {
    const t = transformRef.current;
    const pos = canvasPos(e);
    if (!t || !pos) return;
    const hit = hitTest(points, t, viewRef.current, pos.sx, pos.sy, HIT_RADIUS);
    if (hit) {
      onPointMouseDown(hit, e);
      return;
    }
    // 空白部はドラッグでパン
    if (e.button !== 0) return;
    panningRef.current = true;
    setHover(null);
    let last = { x: e.clientX, y: e.clientY };
    const handleMouseMove = (me: MouseEvent) => {
      const dx = me.clientX - last.x;
      const dy = me.clientY - last.y;
      last = { x: me.clientX, y: me.clientY };
      setView((v) => panBy(v, dx, dy));
    };
    const handleMouseUp = () => {
      window.removeEventListener("mousemove", handleMouseMove);
      window.removeEventListener("mouseup", handleMouseUp);
      panningRef.current = false;
    };
    window.addEventListener("mousemove", handleMouseMove);
    window.addEventListener("mouseup", handleMouseUp);
  };

  const handleMouseMove = (e: React.MouseEvent<HTMLCanvasElement>) => {
    if (panningRef.current) return;
    const t = transformRef.current;
    const pos = canvasPos(e);
    if (!t || !pos) {
      setHover(null);
      return;
    }
    const hit = hitTest(points, t, viewRef.current, pos.sx, pos.sy, HIT_RADIUS);
    setHover(hit ? { sx: pos.sx, sy: pos.sy, subject: hit.subject } : null);
  };

  return (
    <div className="relative">
      <canvas
        ref={canvasRef}
        width={800}
        height={800}
        onMouseDown={handleMouseDown}
        onMouseMove={handleMouseMove}
        onMouseLeave={() => setHover(null)}
        onDoubleClick={() => setView(IDENTITY_VIEW)}
        className="bg-white"
        style={{ cursor: hover ? "pointer" : "default" }}
      />
      {hover && (
        <div
          className="pointer-events-none absolute z-10 max-w-xs truncate rounded bg-gray-800 px-2 py-1 text-xs text-white opacity-90"
          style={{ left: hover.sx + 12, top: hover.sy + 12 }}
        >
          {hover.subject}
        </div>
      )}
    </div>
  );
}
```

- [ ] **Step 2: ビルドと全フロントテスト**

Run: `pnpm build && pnpm test`
Expected: ビルド成功・全テスト PASS（mapGeometry の新旧テスト含む）

- [ ] **Step 3: コミット**

```bash
git add src/components/embedding-map/EmbeddingMapCanvas.tsx
git commit -m "feat(ui): マップにホイールズーム・パン・ホバーツールチップを追加"
```

---

### Task 3: 仕上げ — 実機確認と PR

- [ ] **Step 1: 実機確認（設計書 §10「ズーム・パン・ホバーが動作すること」）**

`CI=true pnpm tauri dev` で起動し、マップウィンドウで確認:

1. ホイールでカーソル位置を中心に拡大・縮小する（ページがスクロールしない）
2. 全体表示（k=1）より縮小できない
3. 空白部のドラッグでパンできる。点のドラッグは従来どおり D&D（案件へ割り当て）になる
4. 点にホバーすると件名ツールチップが出て、カーソルが pointer になる。密集部をズームすると個々の点を判別できる
5. ダブルクリックで全体表示に戻る
6. ズーム中でも点クリック → プレビュー、D&D 割り当て、割り当て後の再着色が正しい位置で動く

- [ ] **Step 2: PR 作成**

PRタイトル: `feat(search): 埋め込みマップにズーム・パン・ホバーを追加`
本文: Phase A/B（#238/#240）で見送った操作系の後続である旨、設計書 §4.6・§5・§10 と本計画へのリンク。

```bash
git push -u origin feat/embedding-map-zoom-pan
gh pr create --title "..." --body "..."
```

---

## 意図的に見送るもの（さらに後続）

- **ピンチズーム（トラックパッドジェスチャ）**: wheel イベント（ctrlKey 付き）で来る環境もあるがまず基本のホイールで開始
- **ミニマップ・ズーム率表示**: 需要が見えたら
- **ホバー点の強調描画（輪郭ハイライト）**: ツールチップで判別は足りる。描画パスの複雑化を避ける
- **パン範囲のクランプ**: 画面外へ流しても ダブルクリックで戻れる（YAGNI）

## 完了後

設計書 §10 の確認方法 4 項目（描画・D&D 割り当て・メイン同期・ズーム/パン/ホバー）がすべて揃う。
