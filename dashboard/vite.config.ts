import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

// The build output lands in ./dist, which rust-embed bakes into the Kinetix
// binary at compile time.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  // Assets resolve under /admin/assets/... so the whole SPA lives under the
  // router's /admin/{*path} catch-all (the admin API owns /admin/api/*).
  base: '/admin/',
  build: {
    outDir: 'dist',
    emptyOutDir: true,
  },
  server: {
    port: 3000,
    // Proxy API calls to a locally running Kinetix during development.
    proxy: {
      '/admin/api': 'http://127.0.0.1:8080',
      '/v1': 'http://127.0.0.1:8080',
      '/healthz': 'http://127.0.0.1:8080',
    },
  },
});
