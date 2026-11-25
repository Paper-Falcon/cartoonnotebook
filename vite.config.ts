import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import legacy from "@vitejs/plugin-legacy";

export default defineConfig({
  plugins: [
    react(),
    // QQ/微信内置浏览器（X5 老内核）不支持 ES Module 脚本与较新 API：
    // 生成 nomodule 降级包（SystemJS + core-js 垫片）让它们也能跑
    legacy({ targets: ["chrome >= 61", "safari >= 11"] }),
  ],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    proxy: {
      "/api": "http://127.0.0.1:8787",
    },
  },
  build: {
    target: "es2015", // 兼容 QQ/微信内置浏览器（X5 内核较老），桌面 WebView 不受影响
    chunkSizeWarningLimit: 1200,
  },
});
