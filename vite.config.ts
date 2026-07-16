import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "path";

const host = process.env.TAURI_DEV_HOST;

const ignoredWatchGlobs = [
  "**/src-tauri/**",
  "**/codey/**",
  "**/logs/**",
  "**/dist/**",
];

export default defineConfig(async () => ({
  plugins: [react()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
      // rehype-highlight 已依赖 highlight.js，但未提升到根 node_modules；
      // 详情页编辑器需要直接复用同一份实现做“字符保真”高亮。
      "highlight.js": path.resolve(
        __dirname,
        "node_modules/.pnpm/highlight.js@11.11.1/node_modules/highlight.js",
      ),
    },
  },
  test: {
    globals: true,
    environment: "jsdom",
    include: ["src/**/*.test.{ts,tsx}"],
  },
  clearScreen: false,
  build: {
    // 多页面构建：主窗使用 index.html，独立文档详情窗使用 detail.html，
    // RunSummary Diff 独立窗使用 diff.html。
    rollupOptions: {
      input: {
        main: path.resolve(__dirname, "index.html"),
        detail: path.resolve(__dirname, "detail.html"),
        diff: path.resolve(__dirname, "diff.html"),
        browser: path.resolve(__dirname, "browser.html"),
        computerUseOverlay: path.resolve(__dirname, "computer-use-overlay.html"),
      },
    },
  },
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      ignored: ignoredWatchGlobs,
    },
  },
}));
