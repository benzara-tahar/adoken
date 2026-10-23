/**
 * Every word and every piece of fake data on the homepage ("Instant") lives here, so marketing can iterate
 * without touching markup. All people, orgs, repos and PRs are fictional (Fabrikam / Contoso style).
 *
 * Code lines use a tiny highlight markup: ⟨k|text⟩ where k is one of
 *   k keyword · s string · f function · t type · n number · c comment · p property · o operator
 * (see `hl()` in ./lib.ts). Everything else is plain text.
 */
import { launchList as sharedLaunchList, x, footer as sharedFooter, download, repo } from '../copy';

export { x, download, repo };

export const meta = {
  title: 'adoken: Azure DevOps, twelve milliseconds away',
  description:
    'A native, local-first desktop client for Azure DevOps. Cmd+P over 100,000 items in 12 ms, PR reviews without the spinner, and one quiet inbox instead of an email flood. Free, open source, early macOS preview out now.',
  themeColor: '#07070b',
};

export const nav = {
  links: [
    { href: '#search', label: 'Search' },
    { href: '#review', label: 'Review' },
    { href: '#speed', label: 'Speed' },
    { href: '#themes', label: 'Themes' },
    { href: '#horizon', label: 'Roadmap' },
  ],
  follow: 'Follow the build',
  github: repo.short,
  cta: 'Download',
};

export const switcher = {
  label: 'Design',
  items: [
    { href: '/v1/', label: 'v1' },
    { href: '/', label: 'v2' },
    { href: '/v3/', label: 'v3' },
  ],
};

export const hero = {
  badge: { tag: 'New', text: 'Now open source · MIT' },
  title: ['Everything in Azure DevOps.', 'Twelve milliseconds away.'],
  lede: 'adoken is a native, local-first client for Azure DevOps. Your pull requests, work items, pipelines and people live on your machine, so search, review and triage move at the speed you think. Not the speed of a spinner.',
  /** Above the release-notes form, under the download. */
  notes: 'Hear when the next build lands: one short email per release.',
  follow: `Follow ${x.handle} on X`,
  stat: { value: '12 ms', label: '⌘P over 100,000 items' },
  keysHint: 'Press',
};

export const launchList = {
  ...sharedLaunchList,
  emailLabel: 'Your email address for release notes',
  submit: 'Get release notes',
};

/** The hero app window: sidebar, "waiting on you" list, palette script, and two PR detail views. */
export const app = {
  org: 'Fabrikam',
  orgs: ['Fabrikam', 'Contoso', 'Northwind'],
  user: 'You',
  sidebar: [
    { icon: 'sun', label: 'Today', key: '1' },
    { icon: 'inbox', label: 'Inbox', key: '2', count: 3 },
    { icon: 'pr', label: 'Pull requests', key: '3', count: 7, active: true },
    { icon: 'item', label: 'Work items', key: '5' },
    { icon: 'pipe', label: 'Pipelines', key: '6' },
    { icon: 'search', label: 'Search', key: 'P' },
  ],
  listTitle: 'Waiting on you',
  listMeta: '6 reviews · synced 4 s ago',
  prs: [
    { id: 4821, title: 'Fix login redirect loop on token refresh', repo: 'checkout-web', who: 'ML', hue: 195, age: '2h', votes: 1, state: 'review' },
    { id: 4790, title: 'Add retry budget to sync worker', repo: 'sync-worker', who: 'DO', hue: 18, age: '3d', votes: 0, state: 'stale' },
    { id: 4812, title: 'Passkey sign-in behind feature flag', repo: 'identity-api', who: 'PR', hue: 170, age: '5h', votes: 2, state: 'review' },
    { id: 4806, title: 'Virtualize order history table', repo: 'checkout-web', who: 'TF', hue: 345, age: '9h', votes: 1, state: 'review' },
    { id: 4799, title: 'Drop legacy SOAP client from billing', repo: 'billing-core', who: 'YS', hue: 100, age: '1d', votes: 0, state: 'draft' },
    { id: 4777, title: 'Cache feature flags at the edge', repo: 'edge-gateway', who: 'IM', hue: 48, age: '2d', votes: 3, state: 'review' },
  ],
  mineTitle: 'Your pull requests',
  mineMeta: '1 needs action',
  mine: [
    { id: 4744, title: 'Lazy-load gift card widget', repo: 'checkout-web', tag: 'Merge conflict', tone: 'bad', age: '6d' },
    { id: 4768, title: 'Retry budget dashboard tile', repo: 'ops-portal', tag: 'Auto-complete · 2/2 approved', tone: 'ok', age: '1d' },
  ],
  palette: {
    placeholder: 'Search PRs, work items, pipelines, people, commands…',
    indexed: '100,000 items',
    scopes: ['! PRs', '# Work items', '@ People', '> Commands'],
    hints: [
      { keys: ['↑', '↓'], label: 'Move' },
      { keys: ['↵'], label: 'Open' },
      { keys: ['⌘', '↵'], label: 'In browser' },
    ],
    didYouMean: 'Typo fixed: showing results for',
    pasted: 'Pasted link',
    /** The typed query, one keystroke per step, with the scripted match count and search time. */
    typed: [
      { q: 'l', count: 41207, ms: 9 },
      { q: 'lg', count: 6318, ms: 11 },
      { q: 'lgo', count: 1152, ms: 8, fix: 'log' },
      { q: 'lgoi', count: 164, ms: 10, fix: 'logi' },
      { q: 'lgoin', count: 38, ms: 12, fix: 'login' },
    ],
    paste: {
      q: 'https://dev.azure.com/fabrikam/sync/_git/sync-worker/pullrequest/4790',
      count: 1,
      ms: 0.4,
    },
    /** A slice of the 100k corpus. Types: pr · wi · pipe · person · cmd. */
    corpus: [
      { type: 'pr', id: '!4821', title: 'Fix login redirect loop on token refresh', meta: 'checkout-web · Mara Lindqvist · 2h' },
      { type: 'wi', id: '#18342', title: 'Login fails after SSO session timeout', meta: 'Bug · Active · Identity' },
      { type: 'pipe', id: 'login-e2e', title: 'login-e2e · run 20260929.4 failed', meta: 'main · Integration stage · 2m ago' },
      { type: 'pr', id: '!4812', title: 'Passkey sign-in behind feature flag', meta: 'identity-api · Priya Raman · 5h' },
      { type: 'person', id: '@lena', title: 'Lena Ogin', meta: 'Identity team · reviewer on 4 PRs' },
      { type: 'cmd', id: '>', title: 'Open repository login-service', meta: 'Command · ⌘⇧O' },
      { type: 'wi', id: '#18290', title: 'Login page flashes unstyled content in Safari', meta: 'Bug · New · Web' },
      { type: 'pr', id: '!4790', title: 'Add retry budget to sync worker', meta: 'sync-worker · Dev Okafor · 3d' },
      { type: 'pr', id: '!4783', title: 'Log throttled syncs as warnings', meta: 'sync-worker · Yuki Sato · 1d' },
      { type: 'pipe', id: 'logistics', title: 'logistics-nightly · run 1182 passed', meta: 'main · 41m ago' },
      { type: 'wi', id: '#17611', title: 'Logging: redact tokens from verbose output', meta: 'Task · Active · Platform' },
      { type: 'pr', id: '!4752', title: 'Lighthouse budget gate for checkout', meta: 'checkout-web · Tomás Ferreira · 4d' },
      { type: 'person', id: '@luca', title: 'Luca Grieco', meta: 'Payments team' },
      { type: 'pipe', id: 'lint-gate', title: 'lint-gate · run 8812 passed', meta: 'release/9.4 · 12m ago' },
      { type: 'wi', id: '#18011', title: 'Legal: update cookie banner copy', meta: 'Task · Closed · Web' },
      { type: 'pr', id: '!4744', title: 'Lazy-load gift card widget', meta: 'checkout-web · Ines Moreau · 6d' },
    ],
  },
  /** The PR opened from the palette. `lines`: [kind, code] where kind is ' ', '+' or '-'. */
  details: {
    4821: {
      title: 'Fix login redirect loop on token refresh',
      branch: 'fix/login-redirect-loop',
      target: 'main',
      author: 'Mara Lindqvist',
      who: 'ML',
      hue: 195,
      files: ['src/auth/session.ts', 'src/auth/guard.ts', 'src/routes/login.tsx', 'test/session.spec.ts'],
      stats: '+38 −12',
      thread: { at: 7, who: 'PR', hue: 170, author: 'Priya Raman', text: 'Nice. Could we log the 401 reason before signing out?' },
      lines: [
        [' ', '⟨k|export⟩ ⟨k|async⟩ ⟨k|function⟩ ⟨f|refreshSession⟩(token: ⟨t|Token⟩) {'],
        ['-', '  ⟨k|const⟩ res = ⟨k|await⟩ ⟨f|fetch⟩(AUTH_URL, { method: ⟨s|\'POST\'⟩ });'],
        ['-', '  ⟨k|if⟩ (!res.ok) ⟨k|return⟩ ⟨f|redirect⟩(⟨s|\'/login\'⟩);'],
        ['+', '  ⟨k|const⟩ res = ⟨k|await⟩ ⟨f|fetch⟩(AUTH_URL, {'],
        ['+', '    method: ⟨s|\'POST\'⟩,'],
        ['+', '    headers: { ⟨p|Authorization⟩: ⟨s|`Bearer ${token.refresh}`⟩ },'],
        ['+', '  });'],
        ['+', '  ⟨k|if⟩ (res.status === ⟨n|401⟩) ⟨k|return⟩ ⟨f|signOut⟩({ reason: ⟨s|\'expired\'⟩ });'],
        [' ', '  ⟨k|return⟩ ⟨f|parseSession⟩(⟨k|await⟩ res.⟨f|json⟩());'],
        [' ', '}'],
      ],
    },
    4790: {
      title: 'Add retry budget to sync worker',
      branch: 'feat/retry-budget',
      target: 'main',
      author: 'Dev Okafor',
      who: 'DO',
      hue: 18,
      files: ['src/worker/governor.rs', 'src/worker/poll.rs', 'tests/governor.rs'],
      stats: '+64 −9',
      thread: { at: 5, who: 'YS', hue: 100, author: 'Yuki Sato', text: 'Budget of 10 matches the TSTU doc. Ship it.' },
      lines: [
        [' ', '⟨k|impl⟩ ⟨t|Governor⟩ {'],
        ['-', '    ⟨k|pub fn⟩ ⟨f|allow⟩(&⟨k|self⟩) -> ⟨t|bool⟩ { ⟨k|true⟩ }'],
        ['+', '    ⟨c|/// Keeps polling under 10 TSTU per 5 minutes.⟩'],
        ['+', '    ⟨k|pub fn⟩ ⟨f|allow⟩(&⟨k|mut self⟩, cost: ⟨t|u32⟩) -> ⟨t|bool⟩ {'],
        ['+', '        ⟨k|self⟩.window.⟨f|expire⟩(⟨t|Instant⟩::⟨f|now⟩());'],
        ['+', '        ⟨k|if⟩ ⟨k|self⟩.window.spent + cost > ⟨n|10⟩ { ⟨k|return false⟩; }'],
        ['+', '        ⟨k|self⟩.window.spent += cost;'],
        ['+', '        ⟨k|true⟩'],
        ['+', '    }'],
        [' ', '}'],
      ],
    },
  },
};

export const proof = [
  { value: '12 ms', label: 'Search over 100k items' },
  { value: '305 ms', label: 'Cold start to your queue' },
  { value: '16 ms', label: 'Every frame, every scroll' },
  { value: '0', label: 'Servers between you and ADO' },
];

export const bento = {
  eyebrow: 'The daily loop',
  title: 'Everything you do in ADO. Minus the waiting.',
  lede: 'Reviews, notifications, nudges and pipelines, all synced to your disk and a keystroke apart.',
  review: {
    title: 'Review without the spinner.',
    body: 'Diffs are computed on your machine, virtualized and syntax-highlighted. Threads sit on their lines, iterations compare against what you last saw, and Complete is pinned to the commit you actually reviewed.',
    keys: [['⌘', '⇧', 'D'], ['A']],
    pr: '!4821',
    prTitle: 'Fix login redirect loop on token refresh',
    file: 'src/auth/session.ts',
    iterations: ['Iteration 2 · your review', 'Iteration 3 · 2 new commits'],
    compare: 'Showing only what changed since your review',
    thread: {
      author: 'Mara Lindqvist',
      who: 'ML',
      hue: 195,
      time: '11:02',
      text: 'Should SSO users keep the redirect instead of a hard sign-out?',
      replyWho: 'You',
      reply: 'Good catch. SSO goes through signOut too now, spec added.',
      resolved: 'Resolved',
    },
    vote: 'Approve',
    voted: 'Approved',
    complete: 'Complete',
    pinned: 'Pinned to 9f3c2a1, the commit you reviewed',
    /** Diff line numbers (old, new) then kind and code. */
    lines: [
      [41, 41, ' ', '⟨k|export⟩ ⟨k|async⟩ ⟨k|function⟩ ⟨f|refreshSession⟩(token: ⟨t|Token⟩) {'],
      [42, null, '-', '  ⟨k|if⟩ (!res.ok) ⟨k|return⟩ ⟨f|redirect⟩(⟨s|\'/login\'⟩);'],
      [null, 42, '+', '  ⟨k|if⟩ (res.status === ⟨n|401⟩) {'],
      [null, 43, '+', '    ⟨k|return⟩ ⟨f|signOut⟩({ reason: ⟨s|\'expired\'⟩, sso: token.sso });'],
      [null, 44, '+', '  }'],
      [43, 45, ' ', '  ⟨k|return⟩ ⟨f|parseSession⟩(⟨k|await⟩ res.⟨f|json⟩());'],
      [44, 46, ' ', '}'],
    ] as [number | null, number | null, string, string][],
    threadAfter: 3,
  },
  inbox: {
    title: 'Twenty emails become one row.',
    body: 'Every comment, vote and push on a PR folds into a single grouped card. Drafts stay quiet, quiet hours hold the rest, and snooze brings it back tomorrow at 9.',
    keys: [['S'], ['E']],
    mailLabel: 'Your email, today',
    mails: [
      'Mara Lindqvist commented on pull request 4821',
      'Build 20260929.4 failed',
      'Dev Okafor voted Wait for author on 4821',
      'Mara Lindqvist replied to a comment on 4821',
      'Pull request 4821 was updated with 1 new commit',
      'You were mentioned in pull request 4821',
      'Priya Raman commented on pull request 4821',
      'Build 20260929.5 succeeded',
      'Mara Lindqvist commented on pull request 4821',
    ],
    floodCount: '18 emails',
    grouped: {
      id: '!4821',
      title: 'Fix login redirect loop on token refresh',
      summary: '6 comments · 2 votes · 1 push · 1 mention',
      count: '18',
      why: 'You are a reviewer',
    },
    quiet: { label: 'Quiet hours', value: '19:00 – 08:00', held: '3 held for the morning' },
    snooze: { label: 'Snooze', options: ['1 hour', 'Tomorrow 9:00', 'Until it changes'], picked: 1, done: 'Snoozed until' },
  },
  nudge: {
    title: 'The nudge Microsoft proved works.',
    body: 'PRs waiting on you and your own work gathering dust surface once, when it matters. adoken never pings anyone on your behalf.',
    keys: [['⌘', 'T']],
    toasts: [
      { app: 'adoken', time: 'now', title: 'Waiting on you for 3 days', text: '!4790 Add retry budget to sync worker', actions: ['Review', 'Tomorrow'] },
      { app: 'adoken', time: '2m', title: 'Your PR is gathering dust', text: '!4744 Lazy-load gift card widget · idle 6 days', actions: ['Open', 'Snooze'] },
    ],
    stat: '60%',
    statLabel: 'faster completion on nudged PRs in Microsoft Research’s Nudge study',
  },
  pipeline: {
    title: 'Red builds, not red inboxes.',
    body: 'Runs you own, failing stage first, failing test up front. Everyone else’s green builds stay out of your way.',
    keys: [['⌘', '6']],
    run: 'login-e2e · 20260929.4',
    branch: 'main',
    stages: [
      { name: 'Build', time: '1m 12s' },
      { name: 'Unit', time: '48s' },
      { name: 'Integration', time: '2m 03s', fail: true },
      { name: 'Deploy', time: 'skipped' },
    ],
    failure: 'session.spec.ts › refresh token expires mid-flow',
    log: 'Expected 200, received 401 after 3 retries',
  },
  offline: {
    title: 'Works on a plane.',
    body: 'Everything lives in SQLite on your disk. Read, search and draft comments at 35,000 ft; adoken syncs the moment you land.',
    status: ['Offline', 'Back online'],
    detail: ['Your whole queue is still here', 'Synced 14 changes in 0.8 s'],
    drafts: '3 draft comments saved',
  },
  keychain: {
    title: 'Your token never leaves the Keychain.',
    body: 'Signed in through the az CLI with an org-scoped, least-privilege PAT that lives only in the macOS Keychain. Never on disk, never in logs.',
    rows: [
      { label: 'SQLite store', value: 'no secrets' },
      { label: 'Logs', value: 'redacted' },
      { label: 'Keychain', value: 'adoken · fabrikam' },
    ],
  },
  direct: {
    title: 'No server in the middle.',
    body: 'adoken talks to dev.azure.com straight from your Mac. No vendor cloud to breach, throttle or go down. Every project, one palette.',
    you: 'Your Mac',
    ado: 'dev.azure.com',
    them: 'Someone else’s server',
  },
  keys: {
    title: 'Hands stay on the keys.',
    body: 'Every action lives in ⌘P, and the ones you use a hundred times a day get a single key.',
    list: [
      { keys: ['⌘', 'P'], label: 'Find anything' },
      { keys: ['J', 'K'], label: 'Move' },
      { keys: ['A'], label: 'Approve' },
      { keys: ['N'], label: 'Next file' },
      { keys: ['S'], label: 'Snooze' },
      { keys: ['E'], label: 'Done' },
      { keys: ['?'], label: 'Every shortcut' },
    ],
  },
};

export const speed = {
  eyebrow: 'Speed',
  title: 'The browser tax, itemized.',
  lede: 'Same task, same org. One side waits on a tab, the other reads from your disk.',
  browser: 'Browser',
  adoken: 'adoken',
  rows: [
    { task: 'Open a pull request', browser: 3400, adoken: 38 },
    { task: 'Find a work item you half remember', browser: 8200, adoken: 12 },
    { task: 'Jump to the next file in a diff', browser: 1100, adoken: 4 },
    { task: 'Cold start to your review queue', browser: 6800, adoken: 305 },
  ],
  total: { value: '19 min', label: 'back every day, at 60 lookups a day' },
  note: 'Browser times: stopwatch on office Wi-Fi against a Fabrikam-sized org. adoken times: perf budgets on an M-series Mac.',
};

export const themes = {
  eyebrow: 'Themes',
  title: 'Your palette. Literally.',
  lede: '29 built-in themes, your own background behind frosted panels, and optional UI sounds. Every theme is contrast-checked and every animation respects Reduce Motion.',
  more: '+21 more',
  list: [
    { id: 'adoken', name: 'adoken Night' },
    { id: 'catppuccin', name: 'Catppuccin Mocha' },
    { id: 'tokyo', name: 'Tokyo Night' },
    { id: 'nord', name: 'Nord' },
    { id: 'github', name: 'GitHub Light' },
    { id: 'rose', name: 'Rosé Pine' },
    { id: 'gruvbox', name: 'Gruvbox Dark' },
    { id: 'dracula', name: 'Dracula' },
  ],
  toggles: [
    { label: 'Background', value: 'Dusk dunes' },
    { label: 'UI sounds', value: 'On' },
    { label: 'Reduce motion', value: 'System' },
  ],
  inbox: [
    { who: 'ML', hue: 195, title: 'Mara commented on !4821', meta: 'checkout-web · 2m', unread: true },
    { who: 'DO', hue: 18, title: '!4790 waiting on you for 3 days', meta: 'Nudge · sync-worker', unread: true },
    { who: 'CI', hue: 350, title: 'login-e2e failed on main', meta: 'Pipeline · 12m' },
    { who: 'PR', hue: 170, title: 'Priya approved !4812', meta: 'identity-api · 1h' },
  ],
  code: [
    ['+', '⟨k|const⟩ ⟨f|budget⟩ = ⟨k|new⟩ ⟨t|Governor⟩({ tstu: ⟨n|10⟩ });'],
    [' ', '⟨c|// poll inside the budget, never throttled⟩'],
    ['-', '⟨k|await⟩ ⟨f|poll⟩(org, { every: ⟨s|\'5s\'⟩ });'],
    ['+', '⟨k|await⟩ budget.⟨f|poll⟩(org, { every: ⟨s|\'60s\'⟩ });'],
  ],
};

export const horizon = {
  eyebrow: 'Roadmap',
  title: 'Where this is going.',
  lede: 'Built in public, one milestone at a time. Here’s the road.',
  stages: [
    {
      when: 'v0.1',
      name: 'Public beta',
      status: 'In training',
      items: ['⌘P over everything', 'PR review, votes, Complete', 'Inbox, snooze, quiet hours', 'Smart nudges', '29 themes and backgrounds'],
    },
    {
      when: 'v0.2',
      name: 'Depth',
      items: ['Changes since my last review', 'Global summon hotkey', 'Multi-line suggestions', 'az sign-in, no PAT at all'],
    },
    {
      when: 'v0.3',
      name: 'Windows',
      items: ['Same app, same budgets', 'Native Windows 11 build', 'Toast notifications'],
    },
    {
      when: 'Later',
      name: 'Team',
      items: ['Team views and review load', 'Keyboard-only review flow', 'Pipeline re-runs'],
    },
  ],
};

export const closing = {
  title: ['Stop waiting on', 'Azure DevOps.'],
  lede: 'Grab the early macOS preview, get one email per release, or watch it get built one commit at a time.',
  follow: `Follow ${x.handle} on X`,
};

export const footer = {
  made: `Designed and built in public by ${x.handle}.`,
  source: 'Source on GitHub (MIT)',
  legal: sharedFooter.legal,
  privacy: 'Privacy',
  mock: 'Every window on this page is a hand-built HTML mock with fictional data.',
};
