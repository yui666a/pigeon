import { describe, it, expect, beforeEach, vi, afterEach } from "vitest";
import { useErrorStore } from "../../stores/errorStore";

describe("errorStore", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    useErrorStore.setState({ toasts: [] });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("addError adds an error toast to the list", () => {
    useErrorStore.getState().addError("Something went wrong");

    const toasts = useErrorStore.getState().toasts;
    expect(toasts).toHaveLength(1);
    expect(toasts[0].message).toBe("Something went wrong");
    expect(toasts[0].kind).toBe("error");
    expect(toasts[0].id).toBeDefined();
    expect(toasts[0].timestamp).toBeDefined();
  });

  it("addSuccess adds a success toast to the list", () => {
    useErrorStore.getState().addSuccess("メールを送信しました");

    const toasts = useErrorStore.getState().toasts;
    expect(toasts).toHaveLength(1);
    expect(toasts[0].message).toBe("メールを送信しました");
    expect(toasts[0].kind).toBe("success");
    expect(toasts[0].id).toBeDefined();
    expect(toasts[0].timestamp).toBeDefined();
  });

  it("adds multiple toasts preserving order and kind", () => {
    useErrorStore.getState().addError("Error 1");
    useErrorStore.getState().addSuccess("Success 1");

    const toasts = useErrorStore.getState().toasts;
    expect(toasts).toHaveLength(2);
    expect(toasts[0]).toMatchObject({ message: "Error 1", kind: "error" });
    expect(toasts[1]).toMatchObject({ message: "Success 1", kind: "success" });
  });

  it("dismissToast removes the specific toast", () => {
    useErrorStore.getState().addError("Error 1");
    useErrorStore.getState().addSuccess("Success 1");

    const toastId = useErrorStore.getState().toasts[0].id;
    useErrorStore.getState().dismissToast(toastId);

    const toasts = useErrorStore.getState().toasts;
    expect(toasts).toHaveLength(1);
    expect(toasts[0].message).toBe("Success 1");
  });

  it("dismissToast does nothing for non-existent id", () => {
    useErrorStore.getState().addError("Error 1");
    useErrorStore.getState().dismissToast("non-existent-id");

    expect(useErrorStore.getState().toasts).toHaveLength(1);
  });

  it("error toasts auto-dismiss after 5 seconds", () => {
    useErrorStore.getState().addError("Temporary error");

    expect(useErrorStore.getState().toasts).toHaveLength(1);

    vi.advanceTimersByTime(4999);
    expect(useErrorStore.getState().toasts).toHaveLength(1);

    vi.advanceTimersByTime(1);
    expect(useErrorStore.getState().toasts).toHaveLength(0);
  });

  it("success toasts auto-dismiss after 5 seconds", () => {
    useErrorStore.getState().addSuccess("メールを送信しました");

    expect(useErrorStore.getState().toasts).toHaveLength(1);

    vi.advanceTimersByTime(4999);
    expect(useErrorStore.getState().toasts).toHaveLength(1);

    vi.advanceTimersByTime(1);
    expect(useErrorStore.getState().toasts).toHaveLength(0);
  });

  it("auto-dismiss only removes the specific toast", () => {
    useErrorStore.getState().addError("Error 1");

    vi.advanceTimersByTime(2000);
    useErrorStore.getState().addSuccess("Success 1");

    // After 3 more seconds (5 total for Error 1), Error 1 should be gone
    vi.advanceTimersByTime(3000);
    const toasts = useErrorStore.getState().toasts;
    expect(toasts).toHaveLength(1);
    expect(toasts[0].message).toBe("Success 1");

    // After 2 more seconds (5 total for Success 1), it should also be gone
    vi.advanceTimersByTime(2000);
    expect(useErrorStore.getState().toasts).toHaveLength(0);
  });
});
