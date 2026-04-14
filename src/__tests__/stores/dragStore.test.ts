import { describe, it, expect, beforeEach } from "vitest";
import { useDragStore } from "../../stores/dragStore";

describe("dragStore", () => {
  beforeEach(() => {
    useDragStore.setState({
      draggingMailIds: null,
      mouseX: 0,
      mouseY: 0,
      dragLabel: "",
    });
  });

  describe("startDrag", () => {
    it("sets draggingMailIds and dragLabel", () => {
      useDragStore.getState().startDrag(["m1", "m2"], "Test Subject");

      expect(useDragStore.getState().draggingMailIds).toEqual(["m1", "m2"]);
      expect(useDragStore.getState().dragLabel).toBe("Test Subject");
    });
  });

  describe("updatePosition", () => {
    it("sets mouseX and mouseY", () => {
      useDragStore.getState().updatePosition(100, 200);

      expect(useDragStore.getState().mouseX).toBe(100);
      expect(useDragStore.getState().mouseY).toBe(200);
    });
  });

  describe("endDrag", () => {
    it("clears draggingMailIds and dragLabel", () => {
      useDragStore.getState().startDrag(["m1"], "Subject");
      useDragStore.getState().updatePosition(50, 60);

      useDragStore.getState().endDrag();

      expect(useDragStore.getState().draggingMailIds).toBeNull();
      expect(useDragStore.getState().dragLabel).toBe("");
    });
  });

  describe("full drag cycle", () => {
    it("start → update → end", () => {
      const store = useDragStore.getState();

      store.startDrag(["m1"], "件名");
      expect(useDragStore.getState().draggingMailIds).toEqual(["m1"]);

      store.updatePosition(300, 400);
      expect(useDragStore.getState().mouseX).toBe(300);

      store.endDrag();
      expect(useDragStore.getState().draggingMailIds).toBeNull();
    });
  });
});
