/**
 * Color tokens: single source for the page CSS (as custom properties), the inline
 * pixel art, and the generated favicon / OG image.
 */
export const colors = {
  bg: '#0b0b10',
  surface: '#14141d',
  line: '#2a2a3d',
  text: '#f4f1ea',
  muted: '#aaa6bc',
  punch: '#ff5a36',
  punchHi: '#ffb199',
  punchLo: '#b8321c',
  ink: '#1a0d12',
  volt: '#ffd23f',
  voltHi: '#fff1a8',
  ado: '#4cc9ff',
} as const;

export type ColorToken = keyof typeof colors;
/** `:root{--bg:#0b0b10;--punch-hi:#ffb199;...}` */
export const rootVars = `:root{${Object.entries(colors)
  .map(([k, v]) => `--${k.replace(/[A-Z]/g, (ch) => `-${ch.toLowerCase()}`)}:${v}`)
  .join(';')}}`;
