import { defineConfig } from "vitest/config";
import vue from "@vitejs/plugin-vue";

export default defineConfig({
  plugins: [vue()],
  // Fixed build info for AboutView (vite.config.ts injects the real ones).
  define: { __APP_COMMIT__: JSON.stringify("abc1234"), __APP_BUILT__: JSON.stringify("2026-11-08T12:00:00Z") },
  test: {
    environment: "jsdom",
    include: ["src/**/*.spec.ts"],
  },
});
