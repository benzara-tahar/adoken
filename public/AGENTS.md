## adoken.dev site

See `README.md` for layout, commands and Cloudflare Pages settings. Short version:

- The homepage is `src/pages/index.astro` built from `src/v2/`; `/v1/` and `/v3/` are alternative designs (`noindex`). Each design keeps every word and all fake mock data in its own `copy.ts`; shared bits (`meta`, `og`, `launchList`, footer legal, privacy/notice pages) in `src/copy.ts`. Don't hard-code copy in components.
- Static assets go in `static/` (configured `publicDir`), not `public/`.
- After changing `meta`/`og` copy, the fist, or colors: `bun run assets` to re-render favicon + OG image.
- Product visuals are hand-built HTML/CSS/TS mocks on fictional data (Contoso/Fabrikam), never real org data. Entrance animations must not start from invisible (see README "Rules of the ring").
- Analytics (PostHog, cookieless) and the launch list (Buttondown): values in `src/integrations.ts`, events via `data-track` (see README "Analytics and launch list"). A new third-party host also goes into the CSP in `static/_headers`, and `/privacy/` copy must keep matching the setup.
- Verify with `bun run build`, then screenshot `bun run preview` at desktop and mobile widths.
- Dev server in background mode: `bunx --bun astro dev --background`; manage with `astro dev stop|status|logs`.
- Astro docs: https://docs.astro.build
