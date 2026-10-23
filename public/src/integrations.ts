/**
 * The two services adoken.dev talks to. Every value here ships to the browser and is public by design.
 * Hosts listed here must also be allowed in `static/_headers` (CSP `connect-src` / `form-action`).
 */

/** PostHog Cloud EU, project "adoken.dev" (cookieless mode is enabled in the project settings). */
export const posthog = {
  // Project token: write-only, meant for client code.
  token: 'phc_zhuZgoeZMFM8HsmjdiLf4GXtxNE9WccUxNMxmQa2QBhC',
  // PostHog managed reverse proxy: CNAME e.adoken.dev (DNS only) → PostHog EU. First-party, so ad-blockers keep it.
  apiHost: 'https://e.adoken.dev',
  uiHost: 'https://eu.posthog.com',
} as const;

/** Buttondown newsletter "adoken": the launch list. Double opt-in; redirects to /subscribed/ and /confirmed/. */
export const buttondown = {
  subscribeUrl: 'https://buttondown.com/api/emails/embed-subscribe/adoken',
} as const;
