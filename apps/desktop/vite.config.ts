import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig(({ mode }) => ({
  plugins: [react()],
  // The screenshot build (--mode mock) swaps the real api module for the one with fixed data
  resolve:
    mode === "mock" ? { alias: [{ find: /^(\.{1,2}\/)api$/, replacement: new URL("./src/api.mock.ts", import.meta.url).pathname }] } : {},
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: { target: "es2022", outDir: mode === "mock" ? "dist-mock" : "dist" },
}));
