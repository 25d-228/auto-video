import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

const developmentServerPort = 1420;

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: developmentServerPort,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  test: {
    environment: "jsdom",
  },
});
