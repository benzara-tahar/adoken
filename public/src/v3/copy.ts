/**
 * Every word on /v3/ ("Ink & Iron", the editorial edition). Marketing: edit freely.
 * Shared facts (X handle, launch-list wording) come from ../copy so the three editions agree.
 * Everything under `demo` is fiction: Contoso / Fabrikam orgs, invented people, repos and PRs.
 */
import { launchList, x } from '../copy';

export { launchList, x };

export const meta = {
  title: 'adoken · Azure DevOps, in one clean stroke',
  description:
    'A native, local-first desktop client for Azure DevOps. Cmd+P over everything, PR review without spinners, notifications that wait until you are ready. Built in public.',
  switcher: { label: 'Edition', items: [['/v1/', 'v1'], ['/', 'v2'], ['/v3/', 'v3']] as const },
} as const;

export const masthead = {
  issue: 'No. 01',
  season: 'Autumn 2026',
  strap: 'A field journal for people who live in Azure DevOps',
  nav: [
    { label: 'The Race', href: '#race' },
    { label: 'Chapters', href: '#chapters' },
    { label: 'Your desk', href: '#desk' },
    { label: 'Colophon', href: '#colophon' },
  ],
  follow: 'Follow the build',
} as const;

export const hero = {
  kicker: 'A native desktop client for Azure DevOps',
  title: ['Azure DevOps,', 'in one clean', 'stroke.'],
  lede: 'adoken is ADO + 拳 (ken, “fist”). A Rust-built, local-first app that keeps your pull requests, work items and pipelines on your own machine, so search answers before you finish typing, diffs open without a spinner, and Azure DevOps only interrupts you when it matters.',
  vertical: '速く、静かに、手元で。',
  verticalGloss: 'Fast, quiet, at hand.',
  ken: { reading: 'ken', gloss: 'fist · n. the hand, closed with intent' },
  facts: [
    ['< 50 ms', 'Cmd+P over 100k items'],
    ['< 500 ms', 'cold start'],
    ['0', 'servers between you and ADO'],
  ],
  figure: 'Fig. 1 · Monday, 09:02. Three reviews waiting; one keystroke to the first. Contoso demo org, Dojo Night ink.',
  follow: 'Follow the build on X',
  platforms: 'macOS first · Windows in v0.3',
} as const;

export const race = {
  eyebrow: '序 · Prologue',
  title: 'The Race',
  dek: 'Same pull request. Same reviewer. Same Monday. On the left, the browser. On the right, adoken. Both start when Mei asks for a review.',
  left: { label: 'The browser', sub: 'dev.azure.com, fourteen tabs deep' },
  right: { label: 'adoken', sub: '⌘P, type, read, approve' },
  leftLog: [
    'Find the email. Which of the 47?',
    'Open the link. Sign in again.',
    'Fourteen tabs. Which one was it?',
    'Files tab. Spinner.',
    'Reload. Spinner.',
    'Still loading the diff…',
  ],
  rightLog: ['⌘P', 'lgoin → login', '↵ Diff from disk', 'Reply inline', 'A · Approve', 'Done.'],
  stamp: '済',
  stampGloss: 'sumi · “done”',
  verdict: 'Approved in 6.8 seconds. The browser is still looking for the Files tab.',
  replay: 'Run it again',
  note: 'A dramatization: the browser clock is fast-forwarded. adoken’s side runs at its real budgets: under 50 ms to search, diffs rendered from disk.',
} as const;

export const quotes = {
  search: {
    text: 'Currently the only Comment related field that can be used in queries is Comment Count.',
    source: 'Developer Community, 2019 · 65 votes, still “New”',
    href: 'https://developercommunity.visualstudio.com/t/query-comments-by-author-mentions-and-content/627871',
  },
  review: {
    text: 'The ADO web experience is depressingly mouse-dependent, particularly when reviewing pull requests.',
    source: 'Developer Community, 2024',
    href: 'https://developercommunity.visualstudio.com/t/ADO-Pull-Request-Keyboard-Shortcuts----N/10725130',
    credit: 'Credit where due: Sprint 261 added shortcuts for PR threads. We want the whole loop.',
  },
  spinner: {
    text: 'There is a spinning icon in place of all code snippets.',
    source: 'Developer Community, 2020',
    href: 'https://developercommunity.visualstudio.com/t/pull-request-overview-is-extremely-slow-showing-co/1118078',
  },
  inbox: {
    text: 'Why did I get this email?',
    source: 'An actual page title in Microsoft’s Azure DevOps docs',
    href: 'https://learn.microsoft.com/en-us/azure/devops/organizations/notifications/troubleshoot-unexpected-email?view=azure-devops',
  },
} as const;

export const chapters = {
  eyebrow: '目次 · Contents',
  title: 'Seven chapters, one loop.',
  intro: 'Search, review, notify, nudge, re-ink, keep it local, play. Each chapter is a thing you do in Azure DevOps every day, done without waiting on a browser tab.',
  toc: [
    ['一', 'Search', '#ch-search'],
    ['二', 'Review', '#ch-review'],
    ['三', 'Calm', '#ch-calm'],
    ['四', 'Nudges', '#ch-nudges'],
    ['五', 'Ink', '#ch-ink'],
    ['六', 'Desk', '#desk'],
    ['七', 'Ken', '#ch-ken'],
  ],
} as const;

export const search = {
  numeral: '一',
  kicker: 'Chapter one · Search',
  title: 'Type what you remember.',
  body: 'Cmd+P searches every pull request, work item, pipeline run, person and command at once. It forgives typos, understands author:@me and type:bug, and opens any dev.azure.com link you paste. The index lives on your disk, so a hundred thousand items answer before your finger leaves the key.',
  points: [
    ['lgoin', 'finds login. Typos are not a search strategy, but they happen.'],
    ['!', 'pull requests only. # work items, @ people, > commands.'],
    ['⌘V', 'paste any ADO link to jump straight to it, no browser.'],
  ],
  stat: { value: '2.9 ms', label: 'median, 100k items, on a laptop' },
  figure: 'Fig. 2 · A typo, forgiven. Results rank by what you touched recently.',
} as const;

export const review = {
  numeral: '二',
  kicker: 'Chapter two · Review',
  title: 'Read the code, not the spinner.',
  body: 'Diffs are computed locally from files already on your disk: virtualized, syntax-highlighted in your theme, fast on a 300-file PR. Threads sit inline and follow the code across pushes. Approve with one key, and Complete is pinned to the commit you actually reviewed, so auto-complete never merges code you have not seen.',
  points: [
    ['Iteration compare', 'Only what changed since your last review, not the whole PR again.'],
    ['Keyboard-only', 'j / k files, n / p hunks, r to reply, a to approve. Mouse optional.'],
    ['Drafts survive', 'Twelve unsent comments outlive a restart, a crash and a flight.'],
    ['Pinned complete', 'Merges the exact commit you approved. New pushes wait for you.'],
  ],
  figure: 'Fig. 3 · Iteration 3 of !4127, diffed against iteration 2. Your reply goes out as you press A.',
} as const;

export const calm = {
  numeral: '三',
  kicker: 'Chapter three · Calm',
  title: 'Twenty emails, folded into one line.',
  body: 'adoken groups every update by what it is about: one row per PR, work item or run, with a count and the reason it reached you. Drafts stay quiet. Quiet hours hold everything until morning and hand you one summary. Snooze until tomorrow 09:00, or until it changes. Native banners only for what you marked urgent.',
  points: [
    ['Grouped', 'One review, one row. Not one email per comment.'],
    ['Quiet hours', '22:00 to 08:00, nothing. Then one summary with your coffee.'],
    ['Why am I seeing this?', 'Every row says: reviewer, mentioned, author, on call.'],
  ],
  figure: 'Fig. 4 · Monday morning. 47 emails in, three rows out.',
  count: ['47 emails', '3 rows'],
} as const;

export const nudges = {
  numeral: '四',
  kicker: 'Chapter four · Nudges',
  title: 'A gentle shove. Once.',
  body: 'A review that has waited two days on you. Your own PR gathering dust. A build that broke on your branch overnight. adoken notices on your machine and nudges you, at most three times a day. It never pings your teammates for you, and it never nags twice about the same thing.',
  stat: {
    value: '60%',
    label: 'faster completion for nudged pull requests',
    source: 'Microsoft Research, “Nudge”, deployed on 147 repositories',
    href: 'https://arxiv.org/pdf/2011.12468',
  },
  aside: 'Microsoft proved nudges work. They just never shipped them to you.',
} as const;

export const theme = {
  numeral: '五',
  kicker: 'Chapter five · Ink',
  title: 'Re-ink the whole thing.',
  body: '33 built-in themes, from Dojo Night to Rosé Pine Dawn, previewed live on the whole window. Import the VS Code theme you already love, set your own background, add soft synthesized UI sounds or keep it silent. Every motion respects Reduce Motion.',
  steps: ['dojo-night', 'dojo-day', 'tokyo-night', 'rose-pine-dawn', 'dracula', 'gruvbox-light', 'nord', 'synthwave'],
  swatchTitle: 'The ink box',
  swatchHint: 'Pick an ink to re-ink the window above.',
  figure: 'Fig. 5 · The same review, eight inks. Scroll to re-ink.',
} as const;

export const desk = {
  numeral: '六',
  kicker: 'Chapter six · Local-first',
  title: 'Your data stays on your desk.',
  body: 'adoken syncs your organizations into SQLite on your own disk and talks straight to dev.azure.com. No vendor server in the middle, no telemetry, no account with us. Your token lives in the OS keychain. On a train with no signal? Search, read and draft anyway; it syncs when you are back.',
  callouts: [
    ['①', 'SQLite, on disk', 'Everything you read comes from here. That is why it is instant.'],
    ['②', 'The keychain', 'Your PAT. Never in the database, never in a log file.'],
    ['③', 'One direct line', 'dev.azure.com and nothing else. Polite polling, inside the rate limits.'],
    ['④', 'Our servers', 'There are none. Nothing to breach, nothing to sunset.'],
  ],
  facts: [
    ['< 500 ms', 'cold start'],
    ['16 ms', 'per frame, budgeted'],
    ['100k', 'items, one search'],
    ['0', 'bytes to us'],
  ],
  figure: 'Fig. 6 · Where your data goes. A short trip.',
  labels: { laptop: 'your Mac', db: 'SQLite', key: 'Keychain', ado: 'dev.azure.com', none: 'vendor cloud' },
} as const;

export const ken = {
  numeral: '七',
  kicker: 'Chapter seven · Ken',
  title: 'Approving is a stamp.',
  body: 'An opt-in arcade layer for people who like a score. Real review work lands hits, hits chain into combos, a merged PR is a K.O., and every Friday you get a fight card. Approve is the Hanko Slam: 判子, the seal you press on a finished document. Off by default, private to your machine, silent under Calm mode.',
  card: {
    title: 'Fight card',
    week: 'Week 39 · Contoso',
    stats: [
      ['34', 'hits'],
      ['×9', 'best combo'],
      ['2', 'K.O.'],
      ['12', 'day streak'],
    ],
    moves: [
      ['Hanko Slam', '判子', 'approve', '11'],
      ['Quick Jab', '突き', 'comment', '17'],
      ['Keen Eye', '目利き', 'file viewed', '40'],
      ['Clean Cut', '断', 'thread resolved', '6'],
    ],
  },
} as const;

export const roadmap = {
  eyebrow: '予定 · What’s next',
  title: 'Printed so far, and the next pages.',
  stamps: { done: '済', next: '次', later: '予' },
  stampLabels: { done: 'done', next: 'next', later: 'planned' },
  items: [
    ['done', 'v0.0.1', 'A native window, four themes, Cmd+P and a perf harness'],
    ['done', 'v0.0.2', 'Connect an org in a minute, least-privilege PAT in the keychain'],
    ['next', 'v0.0.3', 'Sync: your org in SQLite, fresh within a minute'],
    ['later', 'v0.0.5', 'Review for real: line comments, votes, complete, nudges'],
    ['later', 'v0.1', 'Public beta: a signed macOS download'],
    ['later', 'v0.2', 'Iteration compare, global summon hotkey, suggestions'],
    ['later', 'v0.2', 'Team views: who is waiting on whom, at a glance'],
    ['later', 'v0.3', 'Windows 11. Same app, same budgets'],
  ],
  note: 'Targets for a solo builder, not promises. If a page slips, the date moves; the scope does not quietly shrink.',
} as const;

export const colophon = {
  eyebrow: '奥付 · Colophon',
  title: ['Be there for', 'the first printing.'],
  body: 'One email the day v0.1 is ready to download. Or follow along on X for the build log: three posts a week and a short demo every Friday.',
  follow: `Follow ${x.handle} on X`,
  details: [
    ['Set in', 'New York and Hiragino Mincho, or whatever serif your system keeps'],
    ['Built with', 'Rust and GPUI. Native, not a browser in a box'],
    ['Printed on', 'Your own disk. SQLite, a keychain, nothing else'],
    ['Made by', `${x.handle}, in public`],
  ],
  legal:
    'adoken is an independent project, not affiliated with or endorsed by Microsoft. Azure DevOps is a trademark of Microsoft Corporation. All people, orgs and pull requests shown are fictional.',
  privacy: 'Privacy',
} as const;

/** Fictional data for the app mocks. */
export const demo = {
  org: 'Contoso',
  user: 'Alex',
  date: 'Monday, 29 September',
  nav: [
    ['Today', ''],
    ['Inbox', '12'],
    ['Pull requests', '5'],
    ['Work items', ''],
    ['Pipelines', '1'],
  ],
  cards: [
    ['3', 'reviews waiting on you'],
    ['2', 'of your PRs need action'],
    ['1', 'pipeline failing on main'],
  ],
  reviews: [
    ['!4127', 'Retry login when the token refresh races', 'identity', 'Mei Tanaka', '2d'],
    ['!4131', 'Cache price lookups per region', 'storefront-web', 'Jonas Weber', '5h'],
    ['!4119', 'Drop the legacy polyfill bundle', 'storefront-web', 'Priya Raman', '1d'],
  ],
  nudge: 'Mei has waited 2 days on your review of !4127. It blocks the Friday release.',
  query: 'lgoin',
  results: [
    ['pr', '!4127', 'Retry login when the token refresh races', 'identity · Mei Tanaka · 2d'],
    ['bug', '#22039', 'Login loop on Safari after SSO timeout', 'Storefront · Active · P1'],
    ['run', 'identity-ci #1482', 'login-e2e failed on stage “smoke”', 'Pipelines · 38m ago'],
    ['pr', '!4098', 'Move the login form to the design system', 'storefront-web · Luca Bianchi'],
    ['cmd', '>', 'Sign in to another organization', 'Command'],
  ],
  resultsMeta: '5 of 104,382 · 2.9 ms',
  /** Chapter one cycles through these three searches. */
  searches: [
    {
      query: 'lgoin',
      note: 'No exact match for “lgoin”. Showing login.',
      meta: '5 of 104,382 · 2.9 ms',
      rows: [
        ['pr', '!4127', 'Retry login when the token refresh races', 'identity · Mei Tanaka · 2d'],
        ['bug', '#22039', 'Login loop on Safari after SSO timeout', 'Storefront · Active · P1'],
        ['run', 'identity-ci #1482', 'login-e2e failed on stage “smoke”', 'Pipelines · 38m ago'],
        ['pr', '!4098', 'Move the login form to the design system', 'storefront-web · Luca Bianchi'],
      ],
    },
    {
      query: 'author:@me type:bug',
      note: 'Filters: bugs you opened, newest first.',
      meta: '3 of 104,382 · 1.8 ms',
      rows: [
        ['bug', '#22039', 'Login loop on Safari after SSO timeout', 'Storefront · Active · P1'],
        ['bug', '#21877', 'Checkout total rounds down in JPY', 'Payments · Resolved'],
        ['bug', '#21790', 'Diff gutter invisible in high contrast', 'Storefront · New · P3'],
      ],
    },
    {
      query: 'https://dev.azure.com/contoso/identity/_git/identity/pullrequest/4127',
      note: 'Azure DevOps link detected. Opening it here, not in a browser.',
      meta: 'link → !4127 · 0.4 ms',
      rows: [['pr', '!4127', 'Retry login when the token refresh races', 'identity · Mei Tanaka · open in adoken']],
    },
  ],
  pr: {
    id: '!4127',
    title: 'Retry login when the token refresh races',
    branch: 'mei/token-refresh-retry → main',
    author: 'Mei Tanaka',
    files: ['src/auth/session.ts', 'src/auth/retry.ts', 'test/session.test.ts'],
    stats: '+18 −4',
    iteration: 'Since your last review · 2 files',
    file: 'src/auth/session.ts',
    comment: {
      who: 'Mei Tanaka',
      initials: 'MT',
      text: 'Three attempts, 250 ms apart. Matches the gateway’s retry-after.',
    },
    reply: 'Nice. Ship it once the flaky test is quarantined.',
    approved: 'Approved',
    pinned: 'Complete pinned to a1f3c9e · auto-complete on',
  },
  code: `
 export async function refreshSession(client: AuthClient, session: Session) {
-  const token = await client.refresh(session.refreshToken);
-  return { ...session, token };
+  const token = await withRetry(() => client.refresh(session.refreshToken), {
+    attempts: 3,
+    backoffMs: 250,
+  });
+  if (!token) throw new SessionExpiredError(session.userId);
+  return { ...session, token, refreshedAt: Date.now() };
 }`,
  code2: `
+export async function withRetry<T>(fn: () => Promise<T>, opts: RetryOptions) {
+  for (let attempt = 1; attempt <= opts.attempts; attempt++) {
+    try {
+      return await fn();
+    } catch (err) {
+      if (attempt === opts.attempts) throw err;
+      await sleep(opts.backoffMs * attempt);
+    }`,
  emails: [
    '[Azure DevOps] PR 4127: Mei Tanaka commented',
    '[Azure DevOps] PR 4127: Jonas Weber commented',
    'Build failed: storefront-ci #9912',
    '[Azure DevOps] PR 4127: Mei Tanaka pushed 1 commit',
    '[Azure DevOps] Work item 22039 was updated',
    '[Azure DevOps] PR 4127: Mei Tanaka replied',
    'Build failed: storefront-ci #9913',
    '[Azure DevOps] PR 4098 (draft) was updated',
    '[Azure DevOps] PR 4127: Priya Raman voted',
    '[Azure DevOps] Work item 22039: you were mentioned',
    'Build failed: storefront-ci #9914',
    '[Azure DevOps] PR 4127: Mei Tanaka commented',
    'Welcome to Azure DevOps',
    '[Azure DevOps] PR 4131: Jonas Weber created',
  ],
  inbox: [
    ['pr', '!4127 Retry login when the token refresh races', '18 updates · 3 threads need you', 'you are a reviewer'],
    ['run', 'storefront-ci failing on main', '6 runs · since 02:14', 'you own this pipeline'],
    ['bug', '#22039 Login loop on Safari', '2 updates · @mention', 'you were mentioned'],
  ],
  muted: '4 draft updates · muted',
  quiet: { label: 'Quiet hours', range: '22:00 – 08:00', held: '11 held, summarized at 08:00' },
  snooze: ['1 hour', '3 hours', 'Tomorrow, 09:00', 'Monday, 09:00', 'Until it changes'],
  banner: { app: 'adoken', title: '!4127 is waiting on your vote', body: 'Mei Tanaka · 3 threads resolved', actions: ['Open', 'Snooze'] },
  nudgeCards: [
    ['Waiting on you', '!4127 has waited 2 days. Mei is blocked.', 'Review now'],
    ['Gathering dust', 'Your !4102 has had no activity for 6 days.', 'Open !4102'],
    ['Broke overnight', 'storefront-ci failed on your branch at 02:14.', 'Open run'],
  ],
  browserTabs: 14,
  browserUrl: 'dev.azure.com/contoso/identity/_git/identity/pullrequest/4127?_a=files',
} as const;
