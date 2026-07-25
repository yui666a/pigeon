import { useState } from "react";
import type { MapPoint } from "../../types/embeddingMap";

const DRAG_THRESHOLD = 5;

export interface PointDrag {
  point: MapPoint;
  /** ゴースト表示用のマウス位置（clientX/clientY） */
  x: number;
  y: number;
}

/**
 * マップの点のドラッグ。メイン側 useMailDrag と同じ 5px 閾値方式だが、
 * 別ウィンドウでは dragStore（zustand）を共有できないためローカル state で持つ。
 * 閾値未満の mouseup はクリック（onClick）として扱う。
 */
export function usePointDrag(onClick: (point: MapPoint) => void) {
  const [drag, setDrag] = useState<PointDrag | null>(null);

  const startPress = (point: MapPoint, e: React.MouseEvent) => {
    if (e.button !== 0) return;
    const start = { x: e.clientX, y: e.clientY };
    let started = false;

    const handleMouseMove = (me: MouseEvent) => {
      const dx = me.clientX - start.x;
      const dy = me.clientY - start.y;
      if (!started && Math.abs(dx) + Math.abs(dy) > DRAG_THRESHOLD) {
        started = true;
        window.getSelection()?.removeAllRanges();
      }
      if (started) setDrag({ point, x: me.clientX, y: me.clientY });
    };

    const handleMouseUp = () => {
      window.removeEventListener("mousemove", handleMouseMove);
      window.removeEventListener("mouseup", handleMouseUp);
      if (!started) onClick(point);
      // ドロップ先の onMouseUp（React ルート内）は window リスナーより先に
      // 発火するため、ここで消しても取りこぼさない
      setDrag(null);
    };

    window.addEventListener("mousemove", handleMouseMove);
    window.addEventListener("mouseup", handleMouseUp);
  };

  return { drag, startPress };
}
