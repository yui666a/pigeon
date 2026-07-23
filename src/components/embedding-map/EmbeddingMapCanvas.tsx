import { useEffect, useRef } from "react";
import type { MapPoint } from "../../types/embeddingMap";
import { computeBounds, makeTransform, worldToScreen, hitTest, type Transform } from "./mapGeometry";

const PADDING = 40;
const UNASSIGNED_COLOR = "#cccccc";
const DEFAULT_PROJECT_COLOR = "#6b7280";

interface Props {
  points: MapPoint[];
  onPointClick: (mailId: string) => void;
}

export function EmbeddingMapCanvas({ points, onPointClick }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const transformRef = useRef<Transform | null>(null);

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

    // 未分類を先に背面へ（薄いグレー・小さめ）。案件を覆い隠さないため。
    for (const p of points) {
      if (p.project_id) continue;
      const s = worldToScreen(t, p.x, p.y);
      ctx.fillStyle = UNASSIGNED_COLOR;
      ctx.globalAlpha = 0.4;
      ctx.beginPath();
      ctx.arc(s.sx, s.sy, 2, 0, Math.PI * 2);
      ctx.fill();
    }
    // 案件を前面へ（色付き・大きめ）
    for (const p of points) {
      if (!p.project_id) continue;
      const s = worldToScreen(t, p.x, p.y);
      ctx.fillStyle = p.project_color ?? DEFAULT_PROJECT_COLOR;
      ctx.globalAlpha = 0.85;
      ctx.beginPath();
      ctx.arc(s.sx, s.sy, 3.5, 0, Math.PI * 2);
      ctx.fill();
    }
    ctx.globalAlpha = 1;
  }, [points]);

  const handleClick = (e: React.MouseEvent<HTMLCanvasElement>) => {
    const t = transformRef.current;
    const canvas = canvasRef.current;
    if (!t || !canvas) return;
    const rect = canvas.getBoundingClientRect();
    const sx = e.clientX - rect.left;
    const sy = e.clientY - rect.top;
    const hit = hitTest(points, t, sx, sy, 6);
    if (hit) onPointClick(hit.mail_id);
  };

  return (
    <canvas
      ref={canvasRef}
      width={800}
      height={800}
      onClick={handleClick}
      className="bg-white"
    />
  );
}
