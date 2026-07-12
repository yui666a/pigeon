import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/setupTests.ts"],
    // src/ 配下のみを対象にする。エージェント用 git worktree（.claude/worktrees/）や
    // src-tauri/ を誤って収集すると、別ツリーの React が混ざりテストが壊れるため
    include: ["src/**/*.{test,spec}.{ts,tsx}"],
  },
});
