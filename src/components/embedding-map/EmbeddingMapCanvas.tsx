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
