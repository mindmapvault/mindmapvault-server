import { fileURLToPath, URL } from 'url';

import { defineConfig, loadEnv } from 'vite';
import react from '@vitejs/plugin-react';

const envDir = fileURLToPath(new URL('..', import.meta.url));

// loadEnv, not process.env: Vite does not copy .env files into process.env, so
// reading process.env here would ignore the repo-root .env and silently fall
// back to localhost — pointing dev at the wrong backend with no error.
export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, envDir, 'VITE_');

  return {
    envDir: '..',
    base: '/admin/',
    plugins: [react()],
    server: {
      host: '127.0.0.1',
      port: 5275,
      strictPort: true,
      proxy: {
        '/api': {
          target: env.VITE_BACKEND_URL || 'http://localhost:8090',
          changeOrigin: true,
        },
      },
    },
  };
});
