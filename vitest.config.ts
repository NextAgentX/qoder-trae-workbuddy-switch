import path from "node:path";

import { defineConfig } from "vitest/config";

// 只跑前端源码里的单测（`reference/` 是第三方对照仓库，不纳入本项目门禁）。
export default defineConfig({
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
});
