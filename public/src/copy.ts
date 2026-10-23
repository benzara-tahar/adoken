/**
 * Every word on adoken.dev lives here. Marketing: edit freely, then `bun run build`.
 * If you change `meta` or `og`, also run `bun run assets` to re-render the OG image.
 *
 * Inline markup is not supported on purpose: copy stays plain text so it can be
 * reused in the OG image, tweets, and alt text without surprises.
 */

export const x = {
  handle: '@BenzyPrime',
  url: 'https://x.com/BenzyPrime',
} as const;

export const meta = {
  url: 'https://adoken.dev',
  title: 'adoken: Azure DevOps, with a punch',
  description:
    'A native, local-first desktop client for Azure DevOps. Today and smart nudges, an inbox on your terms, side-by-side PR review, work items, pipelines, 33 themes and Cmd+P over 100k items already run on demo data. Built in public.',
  ogImage: '/og.png',
  ogImageAlt:
    'adoken: a pixel-art fist bursting through the screen. ADO + 拳 (ken, fist). Snappy PR reviews, Cmd+P everything, notifications on your terms.',
  themeColor: '#0b0b10',
} as const;

/** The OG image (`bun run assets` renders static/og.png from these). */
export const og = {
  status: 'v0.1 in training · macOS first',
  // Rendered as one word; the second part is highlighted (ado + ken).
  nameParts: ['ado', 'ken'],
  wordplay: {
    left: 'ADO',
    right: '拳',
    rightReading: 'ken',
    rightMeaning: 'fist',
  },
  // Three short beats: the OG image tagline.
  promise: ['Snappy PR reviews.', 'Cmd+P everything.', 'Notifications on your terms.'],
} as const;

/**
 * The launch list (Buttondown, double opt-in). Not a waitlist: nobody queues, nothing is gated.
 * Keep `fine` in line with what the list actually sends (see `privacy`).
 */
export const launchList = {
  emailLabel: 'Get one email when v0.1 is ready to download.',
  placeholder: 'you@company.com',
  submit: 'Email me at launch',
  osLegend: 'Your work machine (optional)',
  os: ['macOS', 'Windows', 'Linux'],
  fine: 'Not a waitlist. One email when v0.1 ships, then short release notes. One-click unsubscribe.',
  privacy: 'Privacy',
} as const;

export const footer = {
  legal:
    'adoken is an independent project, not affiliated with or endorsed by Microsoft. Azure DevOps is a trademark of Microsoft Corporation.',
} as const;

export const recordingConsent = {
  title: 'Help improve adoken.dev?',
  body: 'Allow an optional replay of your clicks, scrolling and page interactions in PostHog? Form inputs are masked. This uses browser storage; anonymous visit counts work either way.',
  privacy: 'Privacy details',
  decline: 'No recording',
  allow: 'Allow recording',
  settings: 'Recording settings',
  enabled: 'Recording is allowed. You can turn it off below.',
  disabled: 'Recording is off. Anonymous visit counts are still enabled.',
  optOut: 'Your browser’s Do Not Track or Global Privacy Control preference disables all analytics and recording.',
  storageUnavailable: 'Your browser blocked storage, so recording stays off.',
} as const;

/** Buttondown redirects here after the form is submitted (email not yet confirmed). */
export const subscribed = {
  eyebrow: 'Press start',
  title: 'Check your inbox.',
  body: 'One more step: open the email we just sent and click the confirm link. Nothing else arrives until you do.',
  cta: 'Back to the title screen',
} as const;

/** Buttondown redirects here after the confirm link is clicked. */
export const confirmed = {
  eyebrow: 'Coin accepted',
  title: 'You’re in.',
  body: 'One email when v0.1 is ready to download, then short release notes. Until then, the build log is on X.',
  cta: `Follow ${x.handle} on X`,
  back: 'Back to the title screen',
} as const;

/**
 * /privacy. Every sentence must match the live setup: PostHog cookieless + proxy (src/integrations.ts,
 * src/scripts/analytics.ts) and the Buttondown settings (double opt-in, reminder, welcome email).
 */
export const privacy = {
  title: 'Privacy',
  updated: 'Updated 2026-09-30',
  intro:
    'adoken.dev is a static site hosted on Cloudflare. Visit counts use cookieless analytics by default. Optional session recording uses browser storage only after you allow it. PostHog provides analytics, and Buttondown holds the launch list if you join it.',
  sections: [
    {
      title: 'Visit counts: PostHog',
      body: [
        'Each page view sends PostHog Cloud EU the page address, the referring site, public campaign tags in the link (ref and utm_*), and your browser, OS and screen size. We also count which sections you reach, approximate active time in each section, scroll milestones, theme and replay clicks, navigation, launch-form starts, work-platform choices, submissions and X-link clicks. We never collect what you type into a form or search demo.',
        'Section time counts only while the page is visible and the window has focus, and pauses after 30 seconds without activity. A random ID groups measurements within one page visit; it is never stored or reused across visits. Email addresses and other private query parameters are removed from analytics URLs. Buttondown’s confirmed subscribers, rather than visits to a confirmation page, measure the launch list.',
        'Without recording consent, unique visitors are counted with a hash of your IP address and browser, salted with a value that changes daily and is then deleted. The IP address itself is discarded. This analytics mode uses no browser storage. We do not create person profiles or track you across sites.',
        'Requests go through e.adoken.dev, PostHog’s managed proxy, which runs on Cloudflare. If your browser sends Do Not Track or Global Privacy Control, nothing is sent at all.',
      ],
      link: { label: 'PostHog privacy policy', href: 'https://posthog.com/privacy' },
    },
    {
      title: 'Optional website recordings',
      body: [
        'On the homepage, you can allow PostHog Cloud EU to reconstruct your page interactions, such as clicks, scrolling and mouse movements. Recording starts only after you allow it. All form inputs are masked, hidden/file inputs and iframes are blocked, and replay URLs have their query strings and fragments removed. Console logs, network requests and payloads, and canvas content are not recorded.',
        'Recordings cover only the public homepage, never signup or confirmation pages or alternate designs. Sessions shorter than 10 seconds are discarded. Recordings are retained for 30 days. They can show how visitors navigate the page, but cannot tell us whether someone found a feature useful.',
        'Your recording choice is remembered in local storage for up to 180 days. If you allow recording, PostHog stores an anonymous identifier and recording state in this tab’s session storage; no analytics cookies are set. Use Recording settings in the homepage footer to change your choice. Turning recording off stops collection and clears the replay storage; previously collected recordings remain until their retention expires. Do Not Track and Global Privacy Control always override recording consent.',
      ],
    },
    {
      title: 'Launch list: Buttondown',
      body: [
        'If you subscribe, Buttondown stores your email address, the work machine you picked (if any), which form you used, and where you came from (the link tag or referring site).',
        'You get a confirmation email first, plus one reminder a day later if you miss it. After you confirm: a short welcome, one email when v0.1 is ready to download, then short release notes. Nothing else.',
        'The list is never sold or shared beyond Buttondown. Every email has a one-click unsubscribe, and unsubscribing stops all mail.',
      ],
      link: { label: 'Buttondown privacy policy', href: 'https://buttondown.com/legal/privacy' },
    },
    {
      title: 'Hosting: Cloudflare',
      body: ['Cloudflare serves this site and, like any web host, processes your IP address to deliver pages and block attacks.'],
      link: { label: 'Cloudflare privacy policy', href: 'https://www.cloudflare.com/privacypolicy/' },
    },
    {
      title: 'Your data, your call',
      body: [
        `To see, correct or delete what the launch list holds about you, reply to any adoken email or message ${x.handle} on X. You can also complain to your data protection authority (in France, the CNIL).`,
      ],
    },
    {
      title: 'The app',
      body: [
        'This page covers the website only. The adoken desktop app sends no usage analytics, and your Azure DevOps token stays in your OS keychain.',
      ],
    },
  ],
  back: 'Back to the title screen',
} as const;

export const notFound = {
  title: 'K.O.',
  body: 'That page got knocked out of the ring.',
  cta: 'Back to the title screen',
} as const;
