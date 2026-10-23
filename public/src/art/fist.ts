import { colors, type ColorToken } from '../theme';

/**
 * Original pixel-art fist (front view, glove + wrist wrap), 21×20 cells.
 * o outline · h highlight · s base · d shade · w wrap · W wrap highlight
 */
const GRID = `
.oooo.oooo.oooo......
ohhhhohhhhohhhhooooo.
ohsssohsssohsssohhhho
ohsssohsssohsssohssso
osssdosssdosssdosssdo
osssdosssdosssdosssdo
oddddoddddoddddoddddo
oooooooooooooodosssdo
ohhhhhhhhhhhhhodsssdo
ossssssssssssssosssdo
osssssssssssssdosssdo
odddddddddddddoddddo.
.oooooooooooooossso..
..osssssssssssssdo...
..odsssssssssssddo...
...oooooooooooooo....
...owwwwwwwwwwwwo....
...oWWWWWWWWWWWWo....
...owwwwwwwwwwwwo....
...oooooooooooooo....
`
  .trim()
  .split('\n');

const CELL_TOKEN: Record<string, ColorToken> = {
  o: 'ink',
  h: 'punchHi',
  s: 'punch',
  d: 'punchLo',
  w: 'volt',
  W: 'voltHi',
};

export const FIST_W = GRID[0].length;
export const FIST_H = GRID.length;

export interface Run {
  x: number;
  y: number;
  w: number;
  token: ColorToken;
}

/** Horizontal runs of same-colored cells: ~4× fewer <rect>s than one per pixel. */
export const fistRuns: Run[] = GRID.flatMap((row, y) => {
  const runs: Run[] = [];
  let x = 0;
  while (x < row.length) {
    const token = CELL_TOKEN[row[x]];
    let end = x + 1;
    while (end < row.length && row[end] === row[x]) end++;
    if (token) runs.push({ x, y, w: end - x, token });
    x = end;
  }
  return runs;
});

/** `<rect>` markup with literal hex fills, for standalone SVG files (favicon, OG). */
export function fistRects(offsetX = 0, offsetY = 0): string {
  return fistRuns
    .map(
      (r) =>
        `<rect x="${r.x + offsetX}" y="${r.y + offsetY}" width="${r.w}" height="1" fill="${colors[r.token]}"/>`,
    )
    .join('');
}
