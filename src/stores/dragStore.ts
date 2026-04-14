import { create } from "zustand";

interface DragState {
  draggingMailIds: string[] | null;
  /** Mouse position during drag — used for ghost element */
  mouseX: number;
  mouseY: number;
  /** Subject text shown in ghost */
  dragLabel: string;
  startDrag: (mailIds: string[], label: string) => void;
  updatePosition: (x: number, y: number) => void;
  endDrag: () => void;
}

export const useDragStore = create<DragState>((set) => ({
  draggingMailIds: null,
  mouseX: 0,
  mouseY: 0,
  dragLabel: "",
  startDrag: (mailIds, label) =>
    set({ draggingMailIds: mailIds, dragLabel: label }),
  updatePosition: (x, y) => set({ mouseX: x, mouseY: y }),
  endDrag: () => set({ draggingMailIds: null, dragLabel: "" }),
}));
