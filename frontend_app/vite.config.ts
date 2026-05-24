/// <reference types="node" />

import { defineConfig, type PluginOption } from 'vite';
import react from '@vitejs/plugin-react';
import { VitePWA } from 'vite-plugin-pwa';
import { fileURLToPath, URL } from 'url';

const pwaPlugin = VitePWA({
  registerType: 'prompt',
  injectRegister: false,
  manifest: {
    id: '/',
    name: 'MindMapVault',
    short_name: 'MindMapVault',
    description: 'Private mind mapping with encrypted vaults.',
    theme_color: '#7c3aed',
    background_color: '#0f172a',
    display: 'standalone',
    start_url: '/',
    scope: '/',
    icons: [
      {
        src: '/icon-512.svg',
        sizes: '512x512',
        type: 'image/svg+xml',
        purpose: 'any',
      },
      {
        src: '/icon-512.svg',
        sizes: '512x512',
        type: 'image/svg+xml',
        purpose: 'maskable',
      },
      {
        src: '/apple-touch-icon.png',
        sizes: '180x180',
        type: 'image/png',
      },
    ],
  },
  workbox: {
    navigateFallback: '/index.html',
    globPatterns: ['**/*.{js,css,html,ico,png,svg,webp,woff2}'],
    runtimeCaching: [
      {
        urlPattern: /^https?:\/\/.*\/api\/.*$/,
        handler: 'NetworkOnly',
      },
    ],
  },
  devOptions: {
    enabled: false,
  },
}) as unknown as PluginOption;

// https://vitejs.dev/config/
export default defineConfig({
  envDir: '..',
  plugins: [
    react(),
    pwaPlugin,
  ],
  resolve: {
    alias: {
      '@mindmapvault/connectors': fileURLToPath(new URL('../packages/connectors/src/index.ts', import.meta.url)),
    },
  },
  optimizeDeps: {
    // hash-wasm loads its own WASM files at runtime — exclude from pre-bundling
    exclude: ['hash-wasm'],
  },
  build: {
    target: 'esnext',
  },
  server: {
    host: '127.0.0.1',
    port: 5274,
    strictPort: true,
    proxy: {
      '/api': {
        target: 'http://localhost:8090',
        changeOrigin: true,
      },
    },
  },
});
