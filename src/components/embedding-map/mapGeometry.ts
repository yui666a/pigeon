import type { MapPoint } from "../../types/embeddingMap";

export interface Bounds {
  minX: number;
  maxX: number;
  minY: number;
  maxY: number;
}

export interface Transform {
  scale: number;
  offsetX: number;
  offsetY: number;
  height: number;
}

export function computeBounds(points: MapPoint[]): Bounds {
  let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
  for (const p of points) {
    if (p.x < minX) minX = p.x;
    if (p.x > maxX) maxX = p.x;
    if (p.y < minY) minY = p.y;
    if (p.y > maxY) maxY = p.y;
  }
  return { minX, maxX, minY, maxY };
}

/**
 * world 座標を padding 付きの canvas ボックスへ等方スケールで収める変換を作る。
 * y は画面下向きが正なので上下反転する。
 */
export function makeTransform(
  b: Bounds,
  width: number,
  height: number,
  padding: number,
): Transform {
  const spanX = b.maxX - b.minX || 1;
  const spanY = b.maxY - b.minY || 1;
  const boxW = width - padding * 2;
  const boxH = height - padding * 2;
  const scale = Math.min(boxW / spanX, boxH / spanY);
  // world minX,minY を左下（padding, height-padding）に合わせる
  const offsetX = padding - b.minX * scale;
  const offsetY = padding - b.minY * scale;
  return { scale, offsetX, offsetY, height };
}

export function worldToScreen(t: Transform, x: number, y: number): { sx: number; sy: number } {
  const sx = x * t.scale + t.offsetX;
  // y 反転: world 上方向 → 画面上方向
  const sy = t.height - (y * t.scale + t.offsetY);
  return { sx, sy };
}

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

/**
 * 画面座標(sx,sy) に最も近い点を radius(px) 以内で返す。無ければ null。
 * 点の画面位置は基底変換 t にビュー変換 view を重ねて求める。
 */
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
