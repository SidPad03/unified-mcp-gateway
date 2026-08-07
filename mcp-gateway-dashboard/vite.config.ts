import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import path from 'path'
import pkg from './package.json' with { type: 'json' }

export default defineConfig({
  plugins: [react(), tailwindcss()],
  define: {
    __APP_VERSION__: JSON.stringify(process.env.VITE_APP_VERSION || pkg.version),
  },
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  server: {
    port: 5173,
    proxy: {
      // Must come before '/api' — the first matching prefix wins, and the
      // generic rule would otherwise forward this to :3200/api/prometheus/metrics.
      // Mirrors nginx.conf, which serves the Prometheus endpoint here so it does
      // not shadow the dashboard's own /metrics page. (A bare '/metrics' proxy
      // made a hard refresh on that route 404 in dev.)
      '/api/prometheus/metrics': {
        target: 'http://localhost:3200',
        changeOrigin: true,
        rewrite: () => '/metrics',
      },
      '/api': {
        target: 'http://localhost:3200',
        changeOrigin: true,
        ws: true,
      },
    },
  },
})
