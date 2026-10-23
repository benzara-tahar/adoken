/**
 * Theme palettes for the /v3/ app mocks ("inks"). Seeds only: borders, washes and diff
 * tints are derived in CSS with color-mix(). Dojo Night / Dojo Day / Arcade / Sakura come
 * from docs/design/tokens.md; the community ports are close approximations of their public palettes.
 */
export interface Ink {
  id: string;
  name: string;
  family: string;
  dark: boolean;
  /** app bg, sidebar, surface, text, secondary text, accent, ok, bad, warn, info */
  ui: [string, string, string, string, string, string, string, string, string, string];
  /** keyword, string, number, comment, function, type */
  syntax: [string, string, string, string, string, string];
}

const ink = (
  id: string,
  name: string,
  family: string,
  dark: boolean,
  ui: Ink['ui'],
  syntax: Ink['syntax'],
): Ink => ({ id, name, family, dark, ui, syntax });

export const inks: Ink[] = [
  ink('dojo-night', 'Dojo Night', 'adoken', true,
    ['#0e0f13', '#111217', '#15161c', '#ecedf1', '#a6a9b6', '#f26b3a', '#3fd18a', '#ff5c6c', '#f0b545', '#62a8ff'],
    ['#ff8a5f', '#9bdb8a', '#ffc46b', '#a0a4b3', '#8fb4ff', '#6fd6d6']),
  ink('dojo-day', 'Dojo Day', 'adoken', false,
    ['#f6f6f8', '#efeff3', '#ffffff', '#15161c', '#51545f', '#c24416', '#0f7a4a', '#c8283a', '#94600a', '#1a66c9'],
    ['#a3360b', '#276a19', '#7a4b00', '#565a64', '#1a56bf', '#095f5f']),
  ink('arcade', 'Arcade', 'adoken', true,
    ['#07060b', '#0a0910', '#0d0c14', '#ffffff', '#d9d6ea', '#ff2e88', '#4dff9a', '#ff6b85', '#ffd23f', '#5cc8ff'],
    ['#ff78b6', '#4dff9a', '#ffd23f', '#bdb9d3', '#6ee7ff', '#c9a6ff']),
  ink('sakura', 'Sakura', 'adoken', false,
    ['#fbf6f7', '#f5ecef', '#ffffff', '#2a1a20', '#5e4a53', '#b8235a', '#1d7a4c', '#c0283f', '#8a5a08', '#2562b8'],
    ['#a81e52', '#1d7a4c', '#8a5a08', '#6b5760', '#2f5fb8', '#7a3e9d']),
  ink('latte', 'Catppuccin Latte', 'Catppuccin', false,
    ['#eff1f5', '#e6e9ef', '#f7f8fa', '#4c4f69', '#6c6f85', '#8839ef', '#40a02b', '#d20f39', '#df8e1d', '#1e66f5'],
    ['#8839ef', '#40a02b', '#fe640b', '#7c7f93', '#1e66f5', '#179299']),
  ink('frappe', 'Catppuccin Frappé', 'Catppuccin', true,
    ['#303446', '#292c3c', '#303446', '#c6d0f5', '#a5adce', '#ca9ee6', '#a6d189', '#e78284', '#e5c890', '#8caaee'],
    ['#ca9ee6', '#a6d189', '#ef9f76', '#949cbb', '#8caaee', '#81c8be']),
  ink('macchiato', 'Catppuccin Macchiato', 'Catppuccin', true,
    ['#24273a', '#1e2030', '#24273a', '#cad3f5', '#a5adcb', '#c6a0f6', '#a6da95', '#ed8796', '#eed49f', '#8aadf4'],
    ['#c6a0f6', '#a6da95', '#f5a97f', '#939ab7', '#8aadf4', '#8bd5ca']),
  ink('mocha', 'Catppuccin Mocha', 'Catppuccin', true,
    ['#1e1e2e', '#181825', '#1e1e2e', '#cdd6f4', '#a6adc8', '#cba6f7', '#a6e3a1', '#f38ba8', '#f9e2af', '#89b4fa'],
    ['#cba6f7', '#a6e3a1', '#fab387', '#9399b2', '#89b4fa', '#94e2d5']),
  ink('tokyo-night', 'Tokyo Night', 'Tokyo Night', true,
    ['#1a1b26', '#16161e', '#1a1b26', '#c0caf5', '#a9b1d6', '#7aa2f7', '#9ece6a', '#f7768e', '#e0af68', '#7dcfff'],
    ['#bb9af7', '#9ece6a', '#ff9e64', '#737aa2', '#7aa2f7', '#2ac3de']),
  ink('tokyo-storm', 'Tokyo Night Storm', 'Tokyo Night', true,
    ['#24283b', '#1f2335', '#24283b', '#c0caf5', '#a9b1d6', '#7aa2f7', '#9ece6a', '#f7768e', '#e0af68', '#7dcfff'],
    ['#bb9af7', '#9ece6a', '#ff9e64', '#7a82ad', '#7aa2f7', '#2ac3de']),
  ink('tokyo-day', 'Tokyo Night Day', 'Tokyo Night', false,
    ['#e1e2e7', '#d0d5e3', '#e9e9ed', '#3760bf', '#6172b0', '#2e7de9', '#587539', '#f52a65', '#8c6c3e', '#007197'],
    ['#9854f1', '#587539', '#b15c00', '#848cb5', '#2e7de9', '#007197']),
  ink('dracula', 'Dracula', 'Dracula', true,
    ['#282a36', '#21222c', '#282a36', '#f8f8f2', '#bdc0d6', '#bd93f9', '#50fa7b', '#ff5555', '#ffb86c', '#8be9fd'],
    ['#ff79c6', '#f1fa8c', '#bd93f9', '#7b88c0', '#50fa7b', '#8be9fd']),
  ink('github-light', 'GitHub Light', 'GitHub', false,
    ['#ffffff', '#f6f8fa', '#ffffff', '#1f2328', '#59636e', '#0969da', '#1a7f37', '#d1242f', '#9a6700', '#0969da'],
    ['#cf222e', '#0a3069', '#0550ae', '#59636e', '#8250df', '#953800']),
  ink('github-dark', 'GitHub Dark', 'GitHub', true,
    ['#0d1117', '#010409', '#0d1117', '#e6edf3', '#9198a1', '#2f81f7', '#3fb950', '#f85149', '#d29922', '#58a6ff'],
    ['#ff7b72', '#a5d6ff', '#79c0ff', '#8b949e', '#d2a8ff', '#ffa657']),
  ink('github-dimmed', 'GitHub Dark Dimmed', 'GitHub', true,
    ['#22272e', '#1c2128', '#22272e', '#adbac7', '#768390', '#539bf5', '#57ab5a', '#e5534b', '#c69026', '#539bf5'],
    ['#f47067', '#96d0ff', '#6cb6ff', '#768390', '#dcbdfb', '#f69d50']),
  ink('nord', 'Nord', 'Nord', true,
    ['#2e3440', '#292e39', '#2e3440', '#eceff4', '#d8dee9', '#88c0d0', '#a3be8c', '#bf616a', '#ebcb8b', '#81a1c1'],
    ['#81a1c1', '#a3be8c', '#b48ead', '#7b88a1', '#88c0d0', '#8fbcbb']),
  ink('gruvbox-dark', 'Gruvbox Dark', 'Gruvbox', true,
    ['#282828', '#1d2021', '#282828', '#ebdbb2', '#bdae93', '#fe8019', '#b8bb26', '#fb4934', '#fabd2f', '#83a598'],
    ['#fb4934', '#b8bb26', '#d3869b', '#928374', '#fabd2f', '#8ec07c']),
  ink('gruvbox-light', 'Gruvbox Light', 'Gruvbox', false,
    ['#fbf1c7', '#f2e5bc', '#fbf1c7', '#3c3836', '#665c54', '#af3a03', '#79740e', '#9d0006', '#b57614', '#076678'],
    ['#9d0006', '#79740e', '#8f3f71', '#7c6f64', '#b57614', '#427b58']),
  ink('rose-pine', 'Rosé Pine', 'Rosé Pine', true,
    ['#191724', '#1f1d2e', '#191724', '#e0def4', '#908caa', '#ebbcba', '#9ccfd8', '#eb6f92', '#f6c177', '#c4a7e7'],
    ['#3e8fb0', '#f6c177', '#c4a7e7', '#6e6a86', '#ebbcba', '#9ccfd8']),
  ink('rose-pine-moon', 'Rosé Pine Moon', 'Rosé Pine', true,
    ['#232136', '#2a273f', '#232136', '#e0def4', '#908caa', '#ea9a97', '#9ccfd8', '#eb6f92', '#f6c177', '#c4a7e7'],
    ['#3e8fb0', '#f6c177', '#c4a7e7', '#6e6a86', '#ea9a97', '#9ccfd8']),
  ink('rose-pine-dawn', 'Rosé Pine Dawn', 'Rosé Pine', false,
    ['#faf4ed', '#fffaf3', '#faf4ed', '#575279', '#797593', '#d7827e', '#56949f', '#b4637a', '#ea9d34', '#907aa9'],
    ['#286983', '#ea9d34', '#907aa9', '#9893a5', '#d7827e', '#56949f']),
  ink('one-dark-pro', 'One Dark Pro', 'One Dark', true,
    ['#282c34', '#21252b', '#282c34', '#abb2bf', '#7f848e', '#61afef', '#98c379', '#e06c75', '#e5c07b', '#56b6c2'],
    ['#c678dd', '#98c379', '#d19a66', '#7f848e', '#61afef', '#e5c07b']),
  ink('atom-light', 'Atom One Light', 'One Dark', false,
    ['#fafafa', '#eaeaeb', '#fafafa', '#383a42', '#696c77', '#4078f2', '#50a14f', '#e45649', '#c18401', '#0184bc'],
    ['#a626a4', '#50a14f', '#986801', '#a0a1a7', '#4078f2', '#c18401']),
  ink('night-owl', 'Night Owl', 'Night Owl', true,
    ['#011627', '#01111d', '#011627', '#d6deeb', '#8badc1', '#82aaff', '#22da6e', '#ef5350', '#ffeb95', '#7fdbca'],
    ['#c792ea', '#ecc48d', '#f78c6c', '#637777', '#82aaff', '#ffcb8b']),
  ink('winter', 'Winter is Coming', 'Winter is Coming', true,
    ['#0f1b2e', '#0b1524', '#0f1b2e', '#d6deeb', '#8fa6c4', '#219fd5', '#8dec95', '#f07178', '#e3d8a7', '#6dbdfa'],
    ['#00bff9', '#bcf0c0', '#8dec95', '#5f7e97', '#87aff4', '#6dbdfa']),
  ink('ayu-dark', 'Ayu Dark', 'Ayu', true,
    ['#0b0e14', '#0d1017', '#0b0e14', '#bfbdb6', '#8a8f98', '#e6b450', '#7fd962', '#d95757', '#e6b673', '#59c2ff'],
    ['#ff8f40', '#aad94c', '#d2a6ff', '#636a72', '#ffb454', '#59c2ff']),
  ink('ayu-light', 'Ayu Light', 'Ayu', false,
    ['#fcfcfc', '#f3f4f5', '#fcfcfc', '#5c6166', '#8a9199', '#fa8d3e', '#6cbf43', '#e65050', '#f2ae49', '#399ee6'],
    ['#fa8d3e', '#86b300', '#a37acc', '#adaeb1', '#f2ae49', '#399ee6']),
  ink('palenight', 'Palenight', 'Palenight', true,
    ['#292d3e', '#242837', '#292d3e', '#a6accd', '#8088b3', '#c792ea', '#c3e88d', '#ff5370', '#ffcb6b', '#82aaff'],
    ['#c792ea', '#c3e88d', '#f78c6c', '#676e95', '#82aaff', '#ffcb6b']),
  ink('synthwave', 'SynthWave ’84', 'SynthWave', true,
    ['#262335', '#171520', '#262335', '#ffffff', '#b6b1c8', '#ff7edb', '#72f1b8', '#fe4450', '#fede5d', '#36f9f6'],
    ['#fede5d', '#ff8b39', '#f97e72', '#848bbd', '#36f9f6', '#fe4450']),
  ink('cobalt2', 'Cobalt2', 'Cobalt2', true,
    ['#193549', '#15232d', '#193549', '#ffffff', '#aebfcc', '#ffc600', '#3ad900', '#ff628c', '#ffc600', '#0088ff'],
    ['#ff9d00', '#3ad900', '#ff628c', '#0088ff', '#ffc600', '#80ffbb']),
  ink('andromeda', 'Andromeda', 'Andromeda', true,
    ['#23262e', '#20232b', '#23262e', '#d5ced9', '#a0a1a7', '#00e8c6', '#96e072', '#ee5d43', '#ffe66d', '#7cb7ff'],
    ['#c74ded', '#96e072', '#f39c12', '#a0a1a7', '#ffe66d', '#ee5d43']),
  ink('dark-plus', 'Dark+', 'Dark+/Light+', true,
    ['#1e1e1e', '#252526', '#1e1e1e', '#d4d4d4', '#9d9d9d', '#0078d4', '#89d185', '#f48771', '#cca700', '#3794ff'],
    ['#569cd6', '#ce9178', '#b5cea8', '#6a9955', '#dcdcaa', '#4ec9b0']),
  ink('light-plus', 'Light+', 'Dark+/Light+', false,
    ['#ffffff', '#f3f3f3', '#ffffff', '#1f1f1f', '#616161', '#005fb8', '#388a34', '#e51400', '#bf8803', '#1a85ff'],
    ['#0000ff', '#a31515', '#098658', '#008000', '#795e26', '#267f99']),
];

export const inkById = Object.fromEntries(inks.map((i) => [i.id, i])) as Record<string, Ink>;

/** `--bg:…;--side:…` for an inline style or a `[data-ink]` rule. */
export function inkVars(i: Ink): string {
  const [bg, side, surf, tx, tx2, acc, ok, bad, warn, info] = i.ui;
  const [kw, str, num, com, fn, ty] = i.syntax;
  return Object.entries({ bg, side, surf, tx, tx2, acc, ok, bad, warn, info, kw, str, num, com, fn, ty })
    .map(([k, v]) => `--${k}:${v}`)
    .concat(`color-scheme:${i.dark ? 'dark' : 'light'}`)
    .join(';');
}

/** One stylesheet with a rule per ink, so switching is a single attribute change. */
export const inkCss = inks.map((i) => `[data-ink="${i.id}"]{${inkVars(i)}}`).join('\n');
