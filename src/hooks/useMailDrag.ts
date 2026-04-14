import { useRef } from "react";
import { useDragStore } from "../stores/dragStore";

const DRAG_THRESHOLD = 5;

/**
 * Hook for mail drag behavior with 5px threshold detection.
 * Returns an onMouseDown handler that starts a drag or falls back to onClick.
 */
export function useMailDrag(
  mailIds: string[],
  label: string,
  onClick: () => void,
) {
  const startDrag = useDragStore((s) => s.startDrag);
  const updatePosition = useDragStore((s) => s.updatePosition);
  const isDragging = useRef(false);
  const startPos = useRef({ x: 0, y: 0 });

  const onMouseDown = (e: React.MouseEvent) => {
    if (e.button !== 0) return;
    startPos.current = { x: e.clientX, y: e.clientY };
    isDragging.current = false;

    const handleMouseMove = (me: MouseEvent) => {
      const dx = me.clientX - startPos.current.x;
      const dy = me.clientY - startPos.current.y;
      if (!isDragging.current && Math.abs(dx) + Math.abs(dy) > DRAG_THRESHOLD) {
        isDragging.current = true;
        startDrag(mailIds, label);
        updatePosition(me.clientX, me.clientY);
      }
      if (isDragging.current) {
        updatePosition(me.clientX, me.clientY);
      }
    };

    const handleMouseUp = () => {
      window.removeEventListener("mousemove", handleMouseMove);
      window.removeEventListener("mouseup", handleMouseUp);
      if (!isDragging.current) {
        onClick();
      }
      isDragging.current = false;
    };

    window.addEventListener("mousemove", handleMouseMove);
    window.addEventListener("mouseup", handleMouseUp);
  };

  return { onMouseDown };
}
