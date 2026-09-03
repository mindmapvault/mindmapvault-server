import { defineConfig } from 'vitest/config';

/**
 * Unit tests run in Node, not a browser. What they cover — layout arithmetic
 * and tree operations — is pure, and the one browser dependency
 * (`measureText`, which asks a canvas) is stubbed per test so widths are the
 * same on every machine. Real font metrics would make the numbers a property
 * of the runner rather than of the code.
 *
 * Same shape as `mindmapvault-saas`, deliberately: the two frontends are
 * near-copies and their tooling should not diverge as well.
 */
export default defineConfig({
  test: {
    environment: 'node',
    globals: true,
    include: ['src/**/*.test.ts', 'src/**/__tests__/**/*.test.ts?(x)'],
  },
});
