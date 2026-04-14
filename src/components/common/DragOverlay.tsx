import { useEffect } from "react";
import { useDragStore } from "../../stores/dragStore";

export function DragOverlay() {
  const draggingMailIds = useDragStore((s) => s.draggingMailIds);
  const mouseX = useDragStore((s) => s.mouseX);
  const mouseY = useDragStore((s) => s.mouseY);
  const dragLabel = useDragStore((s) => s.dragLabel);
  const updatePosition = useDragStore((s) => s.updatePosition);
  const endDrag = useDragStore((s) => s.endDrag);

  useEffect(() => {
    if (!draggingMailIds) return;

    const handleMouseMove = (e: MouseEvent) => {
      updatePosition(e.clientX, e.clientY);
    };

    const handleMouseUp = () => {
      endDrag();
    };

    window.addEventListener("mousemove", handleMouseMove);
    window.addEventListener("mouseup", handleMouseUp);
    return () => {
      window.removeEventListener("mousemove", handleMouseMove);
      window.removeEventListener("mouseup", handleMouseUp);
    };
  }, [draggingMailIds, updatePosition, endDrag]);

  if (!draggingMailIds) return null;

  return (
    <div
      className="pointer-events-none fixed z-50 max-w-48 truncate rounded bg-blue-600 px-3 py-1.5 text-xs text-white shadow-lg"
      style={{ top: mouseY + 12, left: mouseX + 12 }}
    >
      {dragLabel}
      {draggingMailIds.length > 1 && (
        <span className="ml-1 rounded-full bg-blue-400 px-1.5">
          {draggingMailIds.length}
        </span>
      )}
    </div>
  );
}
