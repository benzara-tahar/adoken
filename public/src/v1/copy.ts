/**
 * Every word on /v1/ ("Arcade Dojo") lives here, including the fictional data the mock app windows show.
 * Marketing: edit freely. The mock data is invented (fabrikam org, made-up people, repos and PRs);
 * never paste real org data in here.
 *
 * Plain text only: no inline markup. Scripts import `demo` from this file, so keep it lean.
 */
import { launchList as sharedLaunchList, x } from '../copy';

export { x };

export const meta = {
  title: 'adoken · Azure DevOps at arcade speed',
  description:
    'adoken is a native, local-first desktop client for Azure DevOps. ⌘P over 100k items in milliseconds, PR reviews that never spin, and an inbox that hits back. Built in public.',
  themeColor: '#07060d',
} as const;

export const nav = {
  home: 'adoken home',
  items: [
    { label: 'Moves', href: '#moves' },
    { label: 'Bosses', href: '#bosses' },
    { label: 'Hi-scores', href: '#scores' },
    { label: 'Select', href: '#themes' },
    { label: 'Stages', href: '#stages' },
  ],
  cta: 'Follow on X',
} as const;

export const switcher = {
  label: 'Design',
  items: [
    { href: '/v1/', label: 'V1' },
    { href: '/', label: 'V2' },
    { href: '/v3/', label: 'V3' },
  ],
} as const;

export const hero = {
  hud: ['1P', 'Credit 01', 'Stage 0.1 · macOS first'],
  kanji: '拳',
  titleTop: 'Azure DevOps,',
  titleBottom: 'at arcade speed.',
  lede: 'adoken is a native desktop client for Azure DevOps that keeps your PRs, work items, pipelines and mentions on your own machine. ⌘P answers before you finish typing, reviews never spin, and your inbox finally hits back.',
  equation: { left: 'ADO', right: '拳', note: 'ken · Japanese for “fist”' },
  chips: ['Rust + GPUI', 'Local-first', 'Works offline', 'Windows next'],
  follow: 'Follow the build on X',
} as const;

export const launch = {
  emailLabel: 'One email when the macOS beta drops.',
  placeholder: 'you@company.com',
  submit: 'Email me at launch',
  osLegend: 'Your machine',
  os: sharedLaunchList.os,
  fine: 'No waitlist, no spam. One email at launch, then short release notes. Unsubscribe in one click.',
  privacy: 'Privacy',
} as const;

/** The hero cabinet: an animated mock of the app booting and landing a combo. Purely decorative. */
export const cabinet = {
  marquee: 'adoken',
  marqueeSub: '拳 · 1 player',
  label:
    'Animated mock of the adoken app: it boots like an arcade cabinet, opens the ⌘P palette, forgives the typo “lgoin”, opens a login pull request, approves it for a 7-hit combo and celebrates a merged PR.',
  coin: 'Insert coin',
  credit: 'Credit 01',
  bootTitle: 'adoken',
  bootLines: ['Syncing fabrikam / Checkout', '12,408 PRs · 81,760 work items · 6,144 runs', 'Cold start 305 ms'],
  start: 'Press ⌘P to start',
  query: 'lgoin',
  forgiven: 'Typo forgiven',
  forgivenDetail: 'lgoin → login',
  searchMeta: '4 hits · 2.9 ms · 100,312 items',
  hit: 'Hanko Slam',
  hitKanji: '判子',
  combo: '7',
  rank: 'Gale',
  rankKanji: '疾風',
  approved: 'Approved',
  ko: 'K.O.',
  koCaption: '!4830 Add retry budget to ledger sync',
  koSub: 'merged into main · 3 reviewers',
} as const;

export const ticker = [
  '⌘P < 50 ms over 100k items',
  'Cold start < 500 ms',
  '16 ms frames',
  'Local-first SQLite',
  'Works offline',
  'PAT in your Keychain',
  'No vendor server',
  '30 themes',
  'Typos forgiven',
  'macOS now · Windows next',
] as const;

export const moves = {
  eyebrow: 'Move list',
  title: 'Learn four moves. Win every day.',
  intro:
    'Every action is one key away and every screen is already on your disk. Here is the whole fighting style, played live on a fake org.',
  commandCard: {
    title: 'Command card',
    rows: [
      { keys: ['⌘', 'P'], move: 'Palette Strike', does: 'find anything' },
      { keys: ['J', 'K'], move: 'Footwork', does: 'next / previous' },
      { keys: ['⌘', '⇧', 'D'], move: 'Split Stance', does: 'side-by-side diff' },
      { keys: ['C'], move: 'Quick Jab', does: 'comment' },
      { keys: ['A'], move: 'Hanko Slam', does: 'approve' },
      { keys: ['S'], move: 'Snooze Block', does: 'until tomorrow' },
      { keys: ['E'], move: 'Clean Sweep', does: 'mark done' },
      { keys: ['⌘', '⏎'], move: 'Merge Wave', does: 'complete PR' },
      { keys: ['⌘', ','], move: 'Style Select', does: 'themes' },
      { keys: ['?'], move: 'Scroll of Keys', does: 'every shortcut' },
    ],
  },
  items: [
    {
      id: 'palette',
      n: '01',
      type: 'Special',
      name: 'Palette Strike',
      keys: ['⌘', 'P'],
      title: 'Find it before you finish typing it.',
      body: 'One search over PRs, work items, pipeline runs, people, files and commands. Scope with ! # @ >, paste any dev.azure.com link to jump straight to it, and misspell freely: lgoin still lands on login. The index lives on your disk, so answers come back faster than a browser can repaint.',
      stats: [
        ['DMG', '< 50 ms'],
        ['Range', '100k items'],
        ['Bonus', 'typos forgiven'],
      ],
      hint: 'Live demo. Click in and type: lgoin, !retry, #safari, sana, or paste a dev.azure.com link.',
    },
    {
      id: 'review',
      n: '02',
      type: 'Combo',
      name: 'Review Combo',
      keys: ['⌘', '⇧', 'D'],
      title: 'Review at the speed you read.',
      body: 'Diffs computed locally, virtualized and syntax-highlighted, with threads inline where they belong. Compare iterations to see only what changed since your last pass, vote from the keyboard, and Complete stays pinned to the commit you actually reviewed, so auto-complete never merges code you haven’t seen.',
      stats: [
        ['Scroll', '60 fps · 300 files'],
        ['Diff', 'on your CPU'],
        ['Guard', 'pinned complete'],
      ],
    },
    {
      id: 'inbox',
      n: '03',
      type: 'Block',
      name: 'Inbox Block',
      keys: ['S'],
      title: 'Twenty emails in. One row out.',
      body: 'Comments, votes, mentions, work items and runs fold into one row per subject. Priorities float the fires, drafts stay quiet, S snoozes to tomorrow morning, and quiet hours hold everything until you’re back, then hand you one summary. Native banners come with Open and Snooze buttons.',
      stats: [
        ['Grouping', 'per subject'],
        ['Snooze', 'tomorrow 09:00'],
        ['Quiet', 'hours + digest'],
      ],
    },
    {
      id: 'today',
      n: '04',
      type: 'Counter',
      name: 'Nudge Counter',
      keys: ['⌘', '1'],
      title: 'Nothing rots on your watch.',
      body: 'Each morning, one card: reviews waiting on you, your PRs that need a push, pipelines on fire and work items gathering dust. Nudges are computed on your machine, capped at three a day, and adoken never pings a teammate for you.',
      stats: [
        ['Cap', '3 nudges / day'],
        ['Scope', 'PRs · runs · items'],
        ['Noise', 'zero pings sent'],
      ],
      footnote: 'Microsoft Research’s Nudge study: nudged pull requests finished over 60% faster.',
      footnoteLink: 'https://arxiv.org/abs/2011.12468',
      footnoteCta: 'Read the paper',
    },
  ],
} as const;

export const bosses = {
  eyebrow: 'Boss rush',
  title: 'Six bosses. Six K.O.s.',
  intro: 'Every one of these is a real Azure DevOps complaint. adoken was built to knock each of them out.',
  attackLabel: 'Attack',
  counterLabel: 'Counter',
  ko: 'K.O.',
  items: [
    {
      stage: '01',
      name: 'The Email Flood',
      art: 'flood',
      attack: 'One code review, twenty emails. Then a second review.',
      counter: 'Grouped per PR: one row with a count. Email stays off.',
    },
    {
      stage: '02',
      name: 'Spinner of Doom',
      art: 'spinner',
      attack: 'Every hub click waits on a round trip to the cloud.',
      counter: 'It’s all in local SQLite. Views paint in a single frame.',
    },
    {
      stage: '03',
      name: 'The Stale PR',
      art: 'stale',
      attack: 'Nine days waiting on a reviewer who simply forgot.',
      counter: 'Smart nudges put it on top of the right person’s morning.',
    },
    {
      stage: '04',
      name: 'Tab Hydra',
      art: 'tabs',
      attack: 'Fourteen browser tabs to find one pull request.',
      counter: '⌘P. Type half of it, typos included, or paste the link.',
    },
    {
      stage: '05',
      name: 'The Merge Ambush',
      art: 'ambush',
      attack: 'Auto-complete merges a push you never reviewed.',
      counter: 'Complete is pinned to the exact commit you approved.',
    },
    {
      stage: '06',
      name: 'Wrong @Mention',
      art: 'mention',
      attack: 'Tagged by mistake, pinged by every reply forever.',
      counter: 'Mute the thread in one keystroke. It stays muted.',
    },
  ],
} as const;

export const scores = {
  eyebrow: 'Hi-scores',
  title: 'Measured, not promised.',
  head: ['Rank', 'Score', 'Stage', 'Budget'],
  newRecord: 'New record',
  rows: [
    { rank: '1st', value: 305, unit: 'ms', prefix: '', label: 'Cold start to first frame', budget: '500 ms' },
    { rank: '2nd', value: 2.9, unit: 'ms', prefix: '', label: '⌘P over 100k items (p50)', budget: '50 ms' },
    { rank: '3rd', value: 2, unit: 'ms', prefix: '<', label: 'To draw a frame', budget: '16 ms' },
    { rank: '4th', value: 60, unit: 'fps', prefix: '', label: 'Scrolling a 300-file PR', budget: '60 fps' },
    { rank: '5th', value: 0, unit: '', prefix: '', label: 'Servers between you and ADO', budget: '0' },
  ],
  note: 'Measured on an M4 Pro with 100k synthetic items. Budgets are the line we hold; a regression is a bug.',
} as const;

export const themes = {
  eyebrow: 'Style select',
  title: 'Choose your look.',
  intro:
    'Ports of the themes you already love, plus adoken’s own Dojo, Arcade and Sakura. Import a VS Code theme, drop in your own background, turn on synthesized UI sounds. All of it bows to Reduce Motion.',
  player: '1P',
  pick: 'Pick a theme',
  extras: ['Custom backgrounds', 'Import VS Code theme', 'Synth UI sounds', 'Reduce Motion aware'],
  previewLabel: 'Live preview',
} as const;

/** Colors for the theme-select grid and the live preview. Approximations of each theme's palette. */
export const themeList = [
  { name: 'Dojo Night', bg: '#0b0b10', panel: '#14141d', line: '#2a2a3d', text: '#f4f1ea', muted: '#aaa6bc', accent: '#ff5a36', add: '#5bd68a', del: '#ff5a6e', kw: '#ff7a59', str: '#ffd23f', fn: '#4cc9ff', num: '#c49bff' },
  { name: 'Dojo Day', bg: '#f7f3ea', panel: '#ede5d4', line: '#d8ccb4', text: '#1a0d12', muted: '#6d6475', accent: '#e8431f', add: '#1f8a4c', del: '#c62a3a', kw: '#c2331b', str: '#8a6200', fn: '#1a6fb0', num: '#7a3fc0' },
  { name: 'Arcade', bg: '#0a0420', panel: '#140a33', line: '#2c1a5c', text: '#f6ecff', muted: '#a898d4', accent: '#ff2bd6', add: '#3dffb0', del: '#ff4d6d', kw: '#ff2bd6', str: '#ffd23f', fn: '#22e5ff', num: '#b18cff' },
  { name: 'Sakura', bg: '#fff5f7', panel: '#fbe4eb', line: '#f0c9d5', text: '#3a1d2b', muted: '#8a6475', accent: '#e0457b', add: '#2f9e6e', del: '#d2364f', kw: '#c2185b', str: '#9a5b00', fn: '#6a4bc4', num: '#d0457b' },
  { name: 'Catppuccin Mocha', bg: '#1e1e2e', panel: '#181825', line: '#313244', text: '#cdd6f4', muted: '#a6adc8', accent: '#cba6f7', add: '#a6e3a1', del: '#f38ba8', kw: '#cba6f7', str: '#a6e3a1', fn: '#89b4fa', num: '#fab387' },
  { name: 'Catppuccin Frappé', bg: '#303446', panel: '#292c3c', line: '#414559', text: '#c6d0f5', muted: '#a5adce', accent: '#ca9ee6', add: '#a6d189', del: '#e78284', kw: '#ca9ee6', str: '#a6d189', fn: '#8caaee', num: '#ef9f76' },
  { name: 'Catppuccin Latte', bg: '#eff1f5', panel: '#e6e9ef', line: '#ccd0da', text: '#4c4f69', muted: '#6c6f85', accent: '#8839ef', add: '#40a02b', del: '#d20f39', kw: '#8839ef', str: '#40a02b', fn: '#1e66f5', num: '#fe640b' },
  { name: 'Tokyo Night', bg: '#1a1b26', panel: '#16161e', line: '#292e42', text: '#c0caf5', muted: '#787c99', accent: '#7aa2f7', add: '#9ece6a', del: '#f7768e', kw: '#bb9af7', str: '#9ece6a', fn: '#7aa2f7', num: '#ff9e64' },
  { name: 'Tokyo Night Storm', bg: '#24283b', panel: '#1f2335', line: '#3b4261', text: '#c0caf5', muted: '#8089b3', accent: '#7dcfff', add: '#9ece6a', del: '#f7768e', kw: '#bb9af7', str: '#9ece6a', fn: '#7dcfff', num: '#ff9e64' },
  { name: 'Dracula', bg: '#282a36', panel: '#21222c', line: '#44475a', text: '#f8f8f2', muted: '#a0a6c8', accent: '#bd93f9', add: '#50fa7b', del: '#ff5555', kw: '#ff79c6', str: '#f1fa8c', fn: '#50fa7b', num: '#bd93f9' },
  { name: 'GitHub Dark', bg: '#0d1117', panel: '#161b22', line: '#30363d', text: '#e6edf3', muted: '#8d96a0', accent: '#2f81f7', add: '#3fb950', del: '#f85149', kw: '#ff7b72', str: '#a5d6ff', fn: '#d2a8ff', num: '#79c0ff' },
  { name: 'GitHub Light', bg: '#ffffff', panel: '#f6f8fa', line: '#d0d7de', text: '#1f2328', muted: '#656d76', accent: '#0969da', add: '#1a7f37', del: '#cf222e', kw: '#cf222e', str: '#0a3069', fn: '#8250df', num: '#0550ae' },
  { name: 'Nord', bg: '#2e3440', panel: '#3b4252', line: '#4c566a', text: '#eceff4', muted: '#a3acbd', accent: '#88c0d0', add: '#a3be8c', del: '#bf616a', kw: '#81a1c1', str: '#a3be8c', fn: '#88c0d0', num: '#b48ead' },
  { name: 'Gruvbox Dark', bg: '#282828', panel: '#1d2021', line: '#3c3836', text: '#ebdbb2', muted: '#a89984', accent: '#fe8019', add: '#b8bb26', del: '#fb4934', kw: '#fb4934', str: '#b8bb26', fn: '#fabd2f', num: '#d3869b' },
  { name: 'Gruvbox Light', bg: '#fbf1c7', panel: '#f2e5bc', line: '#d5c4a1', text: '#3c3836', muted: '#7c6f64', accent: '#af3a03', add: '#79740e', del: '#9d0006', kw: '#9d0006', str: '#79740e', fn: '#b57614', num: '#8f3f71' },
  { name: 'Rosé Pine', bg: '#191724', panel: '#1f1d2e', line: '#26233a', text: '#e0def4', muted: '#908caa', accent: '#ebbcba', add: '#9ccfd8', del: '#eb6f92', kw: '#31748f', str: '#f6c177', fn: '#ebbcba', num: '#c4a7e7' },
  { name: 'Rosé Pine Moon', bg: '#232136', panel: '#2a273f', line: '#393552', text: '#e0def4', muted: '#908caa', accent: '#ea9a97', add: '#9ccfd8', del: '#eb6f92', kw: '#3e8fb0', str: '#f6c177', fn: '#ea9a97', num: '#c4a7e7' },
  { name: 'Rosé Pine Dawn', bg: '#faf4ed', panel: '#fffaf3', line: '#f2e9e1', text: '#575279', muted: '#797593', accent: '#d7827e', add: '#56949f', del: '#b4637a', kw: '#286983', str: '#ea9d34', fn: '#d7827e', num: '#907aa9' },
  { name: 'One Dark Pro', bg: '#282c34', panel: '#21252b', line: '#3e4451', text: '#abb2bf', muted: '#7f848e', accent: '#61afef', add: '#98c379', del: '#e06c75', kw: '#c678dd', str: '#98c379', fn: '#61afef', num: '#d19a66' },
  { name: 'Night Owl', bg: '#011627', panel: '#0b2942', line: '#1d3b53', text: '#d6deeb', muted: '#7f99b5', accent: '#82aaff', add: '#addb67', del: '#ef5350', kw: '#c792ea', str: '#ecc48d', fn: '#82aaff', num: '#f78c6c' },
  { name: 'SynthWave ’84', bg: '#262335', panel: '#241b2f', line: '#34294f', text: '#ffffff', muted: '#a39fc0', accent: '#ff7edb', add: '#72f1b8', del: '#fe4450', kw: '#fede5d', str: '#ff8b39', fn: '#36f9f6', num: '#f97e72' },
  { name: 'Cobalt2', bg: '#193549', panel: '#15232d', line: '#234e6d', text: '#ffffff', muted: '#9fb4c7', accent: '#ffc600', add: '#3ad900', del: '#ff628c', kw: '#ff9d00', str: '#3ad900', fn: '#ffc600', num: '#ff628c' },
  { name: 'Ayu Dark', bg: '#0b0e14', panel: '#0f131a', line: '#1c212b', text: '#bfbdb6', muted: '#6c7380', accent: '#e6b450', add: '#7fd962', del: '#d95757', kw: '#ff8f40', str: '#aad94c', fn: '#ffb454', num: '#d2a6ff' },
  { name: 'Ayu Light', bg: '#fcfcfc', panel: '#f3f4f5', line: '#e2e4e6', text: '#5c6166', muted: '#8a9199', accent: '#fa8d3e', add: '#6cbf43', del: '#e65050', kw: '#fa8d3e', str: '#86b300', fn: '#f2ae49', num: '#a37acc' },
  { name: 'Solarized Dark', bg: '#002b36', panel: '#073642', line: '#0f4a57', text: '#93a1a1', muted: '#657b83', accent: '#268bd2', add: '#859900', del: '#dc322f', kw: '#859900', str: '#2aa198', fn: '#268bd2', num: '#d33682' },
  { name: 'Solarized Light', bg: '#fdf6e3', panel: '#eee8d5', line: '#ddd6c1', text: '#586e75', muted: '#93a1a1', accent: '#268bd2', add: '#859900', del: '#dc322f', kw: '#859900', str: '#2aa198', fn: '#268bd2', num: '#d33682' },
  { name: 'Monokai Pro', bg: '#2d2a2e', panel: '#221f22', line: '#403e41', text: '#fcfcfa', muted: '#939293', accent: '#ffd866', add: '#a9dc76', del: '#ff6188', kw: '#ff6188', str: '#ffd866', fn: '#a9dc76', num: '#ab9df2' },
  { name: 'Everforest', bg: '#2d353b', panel: '#272e33', line: '#3d484d', text: '#d3c6aa', muted: '#9da9a0', accent: '#a7c080', add: '#a7c080', del: '#e67e80', kw: '#e67e80', str: '#dbbc7f', fn: '#a7c080', num: '#d699b6' },
  { name: 'Kanagawa', bg: '#1f1f28', panel: '#16161d', line: '#2a2a37', text: '#dcd7ba', muted: '#727169', accent: '#7e9cd8', add: '#76946a', del: '#c34043', kw: '#957fb8', str: '#98bb6c', fn: '#7e9cd8', num: '#d27e99' },
  { name: 'Material Palenight', bg: '#292d3e', panel: '#232635', line: '#3a3f58', text: '#bfc7d5', muted: '#8990b3', accent: '#c792ea', add: '#c3e88d', del: '#ff5370', kw: '#c792ea', str: '#c3e88d', fn: '#82aaff', num: '#f78c6c' },
] as const;

export type Theme = (typeof themeList)[number];

export const fightCard = {
  eyebrow: 'Fight card',
  title: 'Your week, scored.',
  intro:
    'Turn on Arcade mode and real review work lands hits, hits chain into combos, and every PR of yours that merges is a K.O. Each Monday you get a fight card to keep, or export as a PNG. Numbers only, unless you say otherwise.',
  week: 'Week of Sep 22',
  tiles: [
    { label: 'Hits', value: '34', sub: 'reviews, votes, replies' },
    { label: 'Best combo', value: '×9', sub: 'Surge 怒涛' },
    { label: 'K.O.s', value: '3', sub: 'your PRs merged' },
    { label: 'Streak', value: '12', sub: 'working days' },
  ],
  minor: [
    ['PRs reviewed', '14'],
    ['First strikes', '2'],
    ['Assists', '5'],
    ['Fastest first review', '11 min'],
  ],
  kosTitle: 'K.O.s this week',
  kos: [
    { id: '!4830', title: 'Add retry budget to ledger sync', when: 'Fri 16:42' },
    { id: '!4796', title: 'Drop legacy tax rounding path', when: 'Wed 11:05' },
    { id: '!4788', title: 'Stream invoice PDFs instead of buffering', when: 'Mon 14:20' },
  ],
  streakTitle: 'Streak',
  days: ['M', 'T', 'W', 'T', 'F'],
  // 3 weeks of working days, oldest first: hits per day (0 = rest day kept by a shield).
  grid: [3, 5, 2, 4, 6, 1, 4, 0, 5, 3, 7, 4, 6, 2, 8],
  actions: ['Copy image', 'Export image…'],
  note: 'Arcade mode is opt-in and off by default. Calm mode hides it for demos and screen shares.',
} as const;

export const dojo = {
  eyebrow: 'Dojo rules',
  title: 'Your machine. Your rules.',
  rules: [
    {
      n: '壱',
      title: 'Your laptop is the server.',
      body: 'Your org syncs into SQLite on your disk. Search, diffs and the inbox keep working on a plane.',
    },
    {
      n: '弐',
      title: 'Your token stays home.',
      body: 'The PAT lives in the OS keychain: never in the database, never in logs. Or skip it and sign in with the az CLI.',
    },
    {
      n: '参',
      title: 'No middleman.',
      body: 'adoken talks to dev.azure.com and nothing else. No vendor cloud, no relay, no copy of your code anywhere.',
    },
    {
      n: '四',
      title: 'It never speaks for you.',
      body: 'Nudges are for your eyes. adoken never comments, pings or emails a teammate on your behalf.',
    },
    {
      n: '五',
      title: 'Motion is a choice.',
      body: 'Reduce Motion is respected everywhere, sounds are opt-in, and Calm mode quiets the arcade on demand.',
    },
  ],
} as const;

export const stages = {
  eyebrow: 'Stage select',
  title: 'The road to Windows and beyond.',
  here: 'You are here',
  items: [
    {
      version: 'v0.0',
      name: 'Training',
      state: 'clear',
      body: 'A native window in 305 ms, sign-in in a minute, your org synced to disk.',
    },
    {
      version: 'v0.1',
      name: 'Public beta',
      state: 'now',
      body: 'The macOS download: ⌘P, review, inbox, nudges and themes.',
    },
    {
      version: 'v0.2',
      name: 'Depth',
      state: 'next',
      body: 'Changes since your last review, a global summon hotkey, multi-line suggestions, az sign-in with no PAT.',
    },
    {
      version: 'v0.3',
      name: 'Windows',
      state: 'next',
      body: 'Same app, same budgets, on Windows 11 x64 and ARM64.',
    },
    {
      version: 'Bonus',
      name: 'Secret stage',
      state: 'bonus',
      body: 'Team review-load views, a keyboard-only review flow, semantic search, Linux.',
    },
  ],
  stateLabels: { clear: 'Clear', now: 'Up next', next: 'Locked', bonus: '???' },
  note: 'Targets for a solo builder, not promises. If a stage slips, the date moves; the scope doesn’t quietly shrink.',
} as const;

export const finale = {
  continueLabel: 'Next round in',
  title: 'Insert coin.',
  body: 'Get one email when the macOS beta is ready to download. Or watch it get built: three posts a week and a demo video every Friday.',
  follow: `Follow ${x.handle} on X`,
} as const;

export const footer = {
  made: `Made by ${x.handle}. Original pixels, no quarters required.`,
  legal:
    'adoken is an independent project, not affiliated with or endorsed by Microsoft. Azure DevOps is a trademark of Microsoft Corporation. Every name, repo and PR shown here is fictional.',
  privacy: 'Privacy',
  top: 'Back to top',
} as const;

/** Fictional org data shared by the mock windows (and the palette script). */
export const demo = {
  org: 'fabrikam',
  project: 'Checkout',
  me: 'Dev Ramos',
  sidebar: [
    { label: 'Today', key: '1', icon: 'sun' },
    { label: 'Inbox', key: '2', icon: 'inbox', badge: '6' },
    { label: 'Pull requests', key: '3', icon: 'pr', badge: '4' },
    { label: 'Work items', key: '5', icon: 'board' },
    { label: 'Pipelines', key: '6', icon: 'pipe' },
    { label: 'Search', key: 'P', icon: 'search' },
  ],
  prs: [
    { id: '!4821', title: 'Fix login redirect loop after SSO timeout', repo: 'identity', who: 'MO', name: 'Mira Okafor', age: '2h', tone: 'c', status: 'ok' },
    { id: '!4830', title: 'Add retry budget to ledger sync', repo: 'ledger-sync', who: 'DR', name: 'Dev Ramos', age: '5h', tone: 'm', status: 'run' },
    { id: '!4817', title: 'Virtualize invoice table rows', repo: 'web-shell', who: 'TR', name: 'Tomás Ruiz', age: '1d', tone: 'a', status: 'ok' },
    { id: '!4809', title: 'Bump OpenTelemetry exporter to 1.9', repo: 'payments-api', who: 'PN', name: 'Priya Natarajan', age: '3d', tone: 'g', status: 'fail' },
    { id: '!4802', title: 'Cache tax rates per region', repo: 'payments-api', who: 'JW', name: 'Jonah Weiss', age: '4d', tone: 'c', status: 'ok' },
    { id: '!4799', title: 'Stream invoice PDFs instead of buffering', repo: 'web-shell', who: 'SK', name: 'Sana Kaur', age: '5d', tone: 'a', status: 'ok' },
  ],
  /** Everything the ⌘P demo can find. `kind`: pr, bug, task, run, person, file, cmd, repo. */
  index: [
    { kind: 'pr', id: '!4821', title: 'Fix login redirect loop after SSO timeout', meta: 'identity · Mira Okafor · 2h' },
    { kind: 'bug', id: '#22039', title: 'Login button unresponsive on Safari 18', meta: 'Active · Sprint 41 · Sana Kaur' },
    { kind: 'run', id: '#812', title: 'identity-login-e2e failed at Integration tests', meta: 'main · 14 min ago' },
    { kind: 'file', id: 'identity', title: 'src/auth/loginFlow.ts', meta: 'identity · main' },
    { kind: 'cmd', id: '', title: 'Sign in to another organization', meta: 'Command' },
    { kind: 'pr', id: '!4830', title: 'Add retry budget to ledger sync', meta: 'ledger-sync · you · 5h' },
    { kind: 'pr', id: '!4826', title: 'Retry idempotency keys on 409', meta: 'payments-api · Jonah Weiss · 1d' },
    { kind: 'pr', id: '!4817', title: 'Virtualize invoice table rows', meta: 'web-shell · Tomás Ruiz · 1d' },
    { kind: 'pr', id: '!4809', title: 'Bump OpenTelemetry exporter to 1.9', meta: 'payments-api · Priya Natarajan · 3d' },
    { kind: 'pr', id: '!4802', title: 'Cache tax rates per region', meta: 'payments-api · Jonah Weiss · 4d' },
    { kind: 'pr', id: '!4799', title: 'Stream invoice PDFs instead of buffering', meta: 'web-shell · Sana Kaur · 5d' },
    { kind: 'pr', id: '!4791', title: 'Retry webhook delivery with jitter', meta: 'notifications · Ade Bello · 6d' },
    { kind: 'pr', id: '!4785', title: 'Remove retry loop from refund worker', meta: 'payments-api · Lukas Brandt · 1w' },
    { kind: 'pr', id: '!4833', title: 'WIP: ledger backfill for March', meta: 'ledger-sync · you · draft' },
    { kind: 'bug', id: '#22107', title: 'Logout does not clear the refresh token', meta: 'Active · Sprint 41 · Mira Okafor' },
    { kind: 'bug', id: '#22064', title: 'Safari 18 drops the checkout cookie', meta: 'New · Sprint 41 · unassigned' },
    { kind: 'bug', id: '#21980', title: 'Invoice totals off by one cent in CHF', meta: 'Active · Sprint 40 · you' },
    { kind: 'task', id: '#22111', title: 'Redact tokens in sync traces', meta: 'Active · Sprint 41 · Ade Bello' },
    { kind: 'task', id: '#22090', title: 'Write runbook for retry budget alerts', meta: 'New · Sprint 41 · you' },
    { kind: 'task', id: '#22012', title: 'Load test the ledger import', meta: 'Closed · Sprint 40 · Lukas Brandt' },
    { kind: 'run', id: '#2025.09.29.4', title: 'identity-ci failed at Integration tests', meta: 'mira/sso-loop · 14 min ago' },
    { kind: 'run', id: '#1418', title: 'payments-api-ci succeeded', meta: 'main · 32 min ago' },
    { kind: 'run', id: '#377', title: 'web-shell-release waiting for approval', meta: 'release/2025.10 · 1h ago' },
    { kind: 'person', id: '@sana', title: 'Sana Kaur', meta: 'Frontend · 3 PRs waiting on her' },
    { kind: 'person', id: '@mira', title: 'Mira Okafor', meta: 'Identity · reviewer on 5 PRs' },
    { kind: 'person', id: '@tomas', title: 'Tomás Ruiz', meta: 'Web shell · 2 PRs open' },
    { kind: 'person', id: '@priya', title: 'Priya Natarajan', meta: 'Payments · on call this week' },
    { kind: 'person', id: '@jonah', title: 'Jonah Weiss', meta: 'Payments · reviewer on 2 PRs' },
    { kind: 'repo', id: 'repo', title: 'identity', meta: 'fabrikam / Checkout · 3 PRs open' },
    { kind: 'repo', id: 'repo', title: 'payments-api', meta: 'fabrikam / Checkout · 5 PRs open' },
    { kind: 'repo', id: 'repo', title: 'ledger-sync', meta: 'fabrikam / Checkout · 2 PRs open' },
    { kind: 'file', id: 'payments-api', title: 'src/retry/budget.ts', meta: 'payments-api · main' },
    { kind: 'file', id: 'identity', title: 'src/auth/session.ts', meta: 'identity · main' },
    { kind: 'cmd', id: '', title: 'Snooze until tomorrow 09:00', meta: 'Command · S' },
    { kind: 'cmd', id: '', title: 'Toggle side-by-side diff', meta: 'Command · ⌘⇧D' },
    { kind: 'cmd', id: '', title: 'Change theme…', meta: 'Command · ⌘,' },
    { kind: 'cmd', id: '', title: 'Start quiet hours now', meta: 'Command' },
    { kind: 'cmd', id: '', title: 'Show fight card', meta: 'Command · Arcade' },
  ],
  kindLabels: {
    pr: 'PR',
    bug: 'Bug',
    task: 'Task',
    run: 'Run',
    person: 'Person',
    file: 'File',
    cmd: 'Cmd',
    repo: 'Repo',
  },
  palette: {
    placeholder: 'Search PRs, work items, runs, people, commands…',
    scopes: [
      ['!', 'PRs'],
      ['#', 'Work items'],
      ['@', 'People'],
      ['>', 'Commands'],
    ],
    script: [
      'lgoin',
      '!retry',
      '#safari',
      'https://dev.azure.com/fabrikam/Checkout/_git/identity/pullrequest/4821',
      'sana',
    ],
    forgiven: 'Typo forgiven',
    link: 'Link detected',
    linkOpen: 'Open pull request',
    empty: 'No hits. Even the typo tolerance gave up.',
    indexed: '100,312 items',
    footer: ['↑↓ move', '⏎ open', '⌘⏎ open in browser', 'esc close'],
  },
  review: {
    id: '!4821',
    title: 'Fix login redirect loop after SSO timeout',
    author: 'Mira Okafor',
    branch: 'mira/sso-loop',
    target: 'main',
    iteration: 'Iteration 3 vs 2',
    since: 'Since your last review',
    files: [
      { path: 'src/auth/loginFlow.ts', state: 'M', counts: '+6 −2', active: true },
      { path: 'src/auth/safeReturnTo.ts', state: 'A', counts: '+14', active: false },
      { path: 'test/loginFlow.test.ts', state: 'M', counts: '+22 −1', active: false },
    ],
    diff: [
      { t: ' ', a: 18, b: 18, code: "import { readSession, exchangeCode } from './session';" },
      { t: '+', a: 0, b: 19, code: "import { safeReturnTo } from './safeReturnTo';" },
      { t: ' ', a: 19, b: 20, code: '' },
      { t: ' ', a: 20, b: 21, code: 'export async function completeLogin(req: Request): Promise<Response> {' },
      { t: ' ', a: 21, b: 22, code: '  const session = await readSession(req);' },
      { t: '-', a: 22, b: 0, code: "  if (!session) return redirect('/login');" },
      { t: '+', a: 0, b: 23, code: '  if (!session || session.expired) {' },
      { t: '+', a: 0, b: 24, code: '    return redirect(`/login?next=${encodeURIComponent(req.url)}`);' },
      { t: '+', a: 0, b: 25, code: '  }' },
      { t: ' ', a: 23, b: 26, code: '  const token = await exchangeCode(session.code, { timeoutMs: 8_000 });' },
      { t: '-', a: 24, b: 0, code: '  return redirect(session.returnTo);' },
      { t: '+', a: 0, b: 27, code: '  // Never bounce back to /login: that was the loop.' },
      { t: '+', a: 0, b: 28, code: '  return redirect(safeReturnTo(session.returnTo), { token });' },
      { t: ' ', a: 25, b: 29, code: '}' },
    ],
    /** The inline thread sits under this diff row index. */
    threadAfter: 12,
    thread: [
      { who: 'TR', name: 'Tomás Ruiz', tone: 'a', when: '1h', text: 'Could safeReturnTo still send us to /login?login?… if returnTo is nested?' },
      { who: 'MO', name: 'Mira Okafor', tone: 'c', when: '40m', text: 'No: it strips any /login prefix and falls back to /. Test added in iteration 3.' },
    ],
    reply: 'Nice catch, both of you. Ship it.',
    replyWho: 'DR',
    votes: ['Approve', 'Approve with suggestions', 'Wait for author', 'Reject'],
    reviewers: [
      { who: 'TR', tone: 'a', vote: 'ok' },
      { who: 'JW', tone: 'c', vote: 'wait' },
    ],
    complete: 'Complete',
    pinned: 'Pinned to a41c9e2, the commit you reviewed',
    hit: 'Hanko Slam',
    hitKanji: '判子',
    combo: '4',
    rank: 'Flurry',
    rankKanji: '連打',
  },
  inbox: {
    title: 'Inbox',
    filters: ['All', 'PRs', 'Mentions', 'Pipelines', 'Work items'],
    emails: 'ADO would have sent',
    emailsCount: '47 emails',
    rowsCount: '5 rows',
    rows: [
      { kind: 'pr', prio: 'high', title: '!4821 Fix login redirect loop after SSO timeout', detail: '6 comments · 2 votes · Mira, Tomás', count: '8', why: 'Reviewer', time: '2m' },
      { kind: 'run', prio: 'high', title: 'identity-ci failed at Integration tests', detail: 'mira/sso-loop · 2 jobs failed', count: '', why: 'You triggered it', time: '14m' },
      { kind: 'mention', prio: '', title: '#22039 Login button unresponsive on Safari 18', detail: 'Sana Kaur: can you repro on 18.1?', count: '3', why: 'Mentioned', time: '38m' },
      { kind: 'pr', prio: '', title: '!4817 Virtualize invoice table rows', detail: 'New iteration · 3 files changed', count: '2', why: 'Reviewer', time: '1h' },
      { kind: 'draft', prio: '', title: '!4833 WIP: ledger backfill for March', detail: 'Draft · stays quiet until published', count: '', why: 'Draft', time: '3h' },
    ],
    snoozeTarget: 3,
    snooze: ['1 hour', '3 hours', 'Tomorrow 09:00', 'Monday 09:00', 'Until it changes'],
    snoozePick: 2,
    snoozed: 'Snoozed until tomorrow 09:00',
    quiet: 'Quiet hours',
    quietWindow: '18:30 – 09:00',
    held: 'held for your 09:00 summary',
    heldCount: '14',
    banner: {
      app: 'adoken',
      title: 'identity-ci failed',
      body: 'Integration tests · 2 jobs · on !4821',
      actions: ['Open', 'Snooze'],
      when: 'now',
    },
  },
  today: {
    title: 'Your day',
    date: 'Monday, Sep 29',
    stats: [
      { value: '10', label: 'reviews waiting on you' },
      { value: '5', label: 'of your PRs need action' },
      { value: '3', label: 'work items gathering dust' },
    ],
    nudgesTitle: 'Nudges',
    nudgesCap: '3 of 3 today',
    nudges: [
      { tone: 'amber', icon: 'clock', title: '!4809 has waited 3 days on your review', detail: 'Priya Natarajan · payments-api', action: 'Review' },
      { tone: 'red', icon: 'x', title: 'Build failed on your PR !4830', detail: 'ledger-sync-ci · Unit tests · 1 failure', action: 'Open run' },
      { tone: 'cyan', icon: 'merge', title: 'Merge conflict on !4826', detail: 'Retry idempotency keys on 409 · 2 files', action: 'Resolve' },
    ],
    pipelinesTitle: 'Pipelines',
    pipelines: [
      { name: 'identity-ci', branch: 'mira/sso-loop', stages: ['ok', 'ok', 'fail', 'skip'], time: '14m' },
      { name: 'ledger-sync-ci', branch: 'dev/retry-budget', stages: ['ok', 'run', 'wait', 'wait'], time: 'now' },
      { name: 'payments-api-ci', branch: 'main', stages: ['ok', 'ok', 'ok', 'ok'], time: '32m' },
    ],
  },
} as const;
