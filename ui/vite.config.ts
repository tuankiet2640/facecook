import { defineConfig, loadEnv } from "vite";
import react from "@vitejs/plugin-react";
import path from "path";

export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, process.cwd(), "");
  const allowedHosts = env.ALLOWED_HOSTS
    ? env.ALLOWED_HOSTS.split(",").map((h) => h.trim())
    : [];

  return {
  plugins: [react()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "src"),
    },
  },
  server: {
    port: 5173,
    allowedHosts,
    proxy: {
      // All /api requests go to the gateway — avoids CORS in dev.
      "/api": {
        target: "http://localhost:8080",
        changeOrigin: true,
      },
      // WebSocket connections go directly to chat-service.
      // The gateway doesn't proxy WebSocket upgrades.
      "/ws": {
        target: "ws://localhost:8084",
        ws: true,
        changeOrigin: true,
        rewrite: (path) => path.replace(/^\/ws/, "/api/v1/chat/ws"),
      },
    },
  },
  build: {
    outDir: "dist",
    sourcemap: true,
  },
  };
});
