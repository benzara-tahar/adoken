// @ts-check
import { defineConfig } from 'astro/config';

// https://astro.build/config
export default defineConfig({
  site: 'https://adoken.dev',
  output: 'static',
  // Static assets live in ./static (not ./public) so the site root, which is
  // itself the repo's `public/` folder, doesn't end up with public/public/.
  publicDir: './static',
  trailingSlash: 'ignore',
  build: {
    // One tiny stylesheet: inline it so first paint needs zero extra requests.
    inlineStylesheets: 'always',
  },
  vite: {
    build: {
      // Never inline scripts: the CSP (static/_headers) is `script-src 'self'` with no hashes, so an
      // inlined <script> would be blocked in production.
      assetsInlineLimit: 0,
    },
  },
});
