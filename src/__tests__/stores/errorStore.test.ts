import { describe, it, expect, beforeEach, vi, afterEach } from "vitest";
import { useErrorStore } from "../../stores/errorStore";

describe("errorStore", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    useErrorStore.setState({ errors: [] });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("addError adds an error to the list", () => {
    useErrorStore.getState().addError("Something went wrong");

    const errors = useErrorStore.getState().errors;
    expect(errors).toHaveLength(1);
    expect(errors[0].message).toBe("Something went wrong");
    expect(errors[0].id).toBeDefined();
    expect(errors[0].timestamp).toBeDefined();
  });

  it("addError adds multiple errors to the list", () => {
    useErrorStore.getState().addError("Error 1");
    useErrorStore.getState().addError("Error 2");

    const errors = useErrorStore.getState().errors;
    expect(errors).toHaveLength(2);
    expect(errors[0].message).toBe("Error 1");
    expect(errors[1].message).toBe("Error 2");
  });

  it("dismissError removes the specific error", () => {
    useErrorStore.getState().addError("Error 1");
    useErrorStore.getState().addError("Error 2");

    const errorId = useErrorStore.getState().errors[0].id;
    useErrorStore.getState().dismissError(errorId);

    const errors = useErrorStore.getState().errors;
    expect(errors).toHaveLength(1);
    expect(errors[0].message).toBe("Error 2");
  });

  it("dismissError does nothing for non-existent id", () => {
    useErrorStore.getState().addError("Error 1");
    useErrorStore.getState().dismissError("non-existent-id");

    expect(useErrorStore.getState().errors).toHaveLength(1);
  });

  it("errors auto-dismiss after 5 seconds", () => {
    useErrorStore.getState().addError("Temporary error");

    expect(useErrorStore.getState().errors).toHaveLength(1);

    vi.advanceTimersByTime(4999);
    expect(useErrorStore.getState().errors).toHaveLength(1);

    vi.advanceTimersByTime(1);
    expect(useErrorStore.getState().errors).toHaveLength(0);
  });

  it("auto-dismiss only removes the specific error", () => {
    useErrorStore.getState().addError("Error 1");

    vi.advanceTimersByTime(2000);
    useErrorStore.getState().addError("Error 2");

    // After 3 more seconds (5 total for Error 1), Error 1 should be gone
    vi.advanceTimersByTime(3000);
    const errors = useErrorStore.getState().errors;
    expect(errors).toHaveLength(1);
    expect(errors[0].message).toBe("Error 2");

    // After 2 more seconds (5 total for Error 2), Error 2 should also be gone
    vi.advanceTimersByTime(2000);
    expect(useErrorStore.getState().errors).toHaveLength(0);
  });
});
