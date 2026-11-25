import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    proxy: {
      "/api": "http://127.0.0.1:8787",
    },
  },
  build: {
    target: "es2017", // 兼容 QQ/微信内置浏览器（X5 内核较老），桌面 WebView 不受影响
    chunkSizeWarningLimit: 1200,
  },
});
