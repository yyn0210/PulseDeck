import { defineConfig } from "vite";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const rootDir = dirname(fileURLToPath(import.meta.url));
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    // Tauri 使用 WebView（Chromium），可直接输出新的 ES 语法
    target: process.env.TAURI_ENV_PLATFORM === "windows" ? "chrome105" : "safari13",
    minify: !process.env.TAURI_ENV_DEBUG ? "esbuild" : false,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
    rollupOptions: {
      // 三个入口：主面板 index.html、桌面宠物 pet.html、任务栏挂件 taskbar.html
      // （后两个窗口都由 Rust 侧按需创建）
      input: {
        main: resolve(rootDir, "index.html"),
        pet: resolve(rootDir, "pet.html"),
        taskbar: resolve(rootDir, "taskbar.html"),
      },
    },
  },
});
