import { describe, it, expect, beforeEach } from "vitest";
import {
  COMPOSE_FORMAT_KEY,
  getDefaultComposeFormat,
  setDefaultComposeFormat,
} from "../../utils/composeFormat";

describe("composeFormat", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("defaults to plain when nothing is stored", () => {
    expect(getDefaultComposeFormat()).toBe("plain");
  });

  it("returns rich only when explicitly stored as rich", () => {
    localStorage.setItem(COMPOSE_FORMAT_KEY, "rich");
    expect(getDefaultComposeFormat()).toBe("rich");
  });

  it("treats any other stored value as plain", () => {
    localStorage.setItem(COMPOSE_FORMAT_KEY, "something-else");
    expect(getDefaultComposeFormat()).toBe("plain");
  });

  it("stores rich and clears the key for plain (plain is the implicit default)", () => {
    setDefaultComposeFormat("rich");
    expect(localStorage.getItem(COMPOSE_FORMAT_KEY)).toBe("rich");
    setDefaultComposeFormat("plain");
    expect(localStorage.getItem(COMPOSE_FORMAT_KEY)).toBeNull();
  });
});
