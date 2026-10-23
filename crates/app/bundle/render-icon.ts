/// <reference types="bun" />
/**
 * Renders the macOS app icon master (crates/app/bundle/icon-1024.png) from the
 * same original pixel fist and color tokens as adoken.dev
 * (public/src/art/fist.ts, public/src/theme.ts). The PNG is committed;
 * scripts/bundle.sh turns it into adoken.icns with sips + iconutil, so
 * bundling never needs Bun.
 *
 * Re-run after changing the fist or the site palette:
 *   (cd public && bun install) && bun crates/app/bundle/render-icon.ts
 *
 * Layout follows Apple's macOS icon grid: 1024 canvas, 824 px rounded tile
 * inset 100 px, soft drop shadow (a blurred copy under the tile) (sips/iconutil can't mask, so the shape is
 * baked in here).
 */
import { Resvg } from '../../../public/node_modules/@resvg/resvg-js';
import { fistRects, FIST_W, FIST_H } from '../../../public/src/art/fist';
import { colors as c } from '../../../public/src/theme';

const SIZE = 1024;
const INSET = 100;
const TILE = SIZE - 2 * INSET;
const RADIUS = 185;
const CELL = 26;
const CX = SIZE / 2;
const CY = SIZE / 2 + 8;

// Impact burst behind the fist: alternating long/short points.
const burst = Array.from({ length: 28 }, (_, i) => {
  const r = i % 2 === 0 ? 350 : 230;
  const a = (Math.PI * 2 * i) / 28 - Math.PI / 2 + 0.06;
  return `${(CX + r * Math.cos(a)).toFixed(1)},${(CY + r * Math.sin(a)).toFixed(1)}`;
}).join(' ');

const fx = CX - (FIST_W * CELL) / 2;
const fy = CY - (FIST_H * CELL) / 2;

const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${SIZE}" height="${SIZE}" viewBox="0 0 ${SIZE} ${SIZE}">
  <defs>
    <linearGradient id="tile" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="${c.surface}"/>
      <stop offset="1" stop-color="${c.bg}"/>
    </linearGradient>
    <radialGradient id="glow" cx="0.5" cy="0.52" r="0.5">
      <stop offset="0" stop-color="${c.punch}" stop-opacity="0.45"/>
      <stop offset="1" stop-color="${c.punch}" stop-opacity="0"/>
    </radialGradient>
    <filter id="blur" x="-10%" y="-10%" width="120%" height="120%">
      <feGaussianBlur stdDeviation="14"/>
    </filter>
    <clipPath id="clip"><rect x="${INSET}" y="${INSET}" width="${TILE}" height="${TILE}" rx="${RADIUS}"/></clipPath>
  </defs>
  <rect x="${INSET}" y="${INSET + 10}" width="${TILE}" height="${TILE}" rx="${RADIUS}" fill="#000" opacity="0.35" filter="url(#blur)"/>
  <rect x="${INSET}" y="${INSET}" width="${TILE}" height="${TILE}" rx="${RADIUS}" fill="url(#tile)"/>
  <g clip-path="url(#clip)">
    <circle cx="${CX}" cy="${CY}" r="420" fill="url(#glow)"/>
    <polygon points="${burst}" fill="${c.punchLo}" opacity="0.28"/>
  </g>
  <rect x="${INSET + 2}" y="${INSET + 2}" width="${TILE - 4}" height="${TILE - 4}" rx="${RADIUS - 2}" fill="none" stroke="${c.line}" stroke-width="4"/>
  <g transform="translate(${fx} ${fy}) scale(${CELL})" shape-rendering="crispEdges">${fistRects()}</g>
</svg>`;

const png = new Resvg(svg, { fitTo: { mode: 'width', value: SIZE } }).render().asPng();
const out = new URL('./icon-1024.png', import.meta.url);
await Bun.write(out, png);
console.log(`wrote ${out.pathname}`);
