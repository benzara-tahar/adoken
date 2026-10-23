/// <reference types="bun" />
/**
 * Renders favicon.svg, favicon.ico, apple-touch-icon.png and og.png into ./static
 * from the same pixel art, tokens and copy the page uses. Outputs are committed;
 * re-run with `bun run assets` after changing copy.ts (og/meta), theme.ts or the fist.
 *
 * Text in the OG image is rasterized with fonts installed on this machine (built on
 * macOS: Helvetica Neue, Menlo, Hiragino Sans), which is why the PNG is committed
 * rather than rendered in CI.
 */
import { Resvg } from '@resvg/resvg-js';
import { fistRects, FIST_W, FIST_H } from '../src/art/fist';
import { colors as c } from '../src/theme';
import { og, meta, x } from '../src/copy';

const out = new URL('../static/', import.meta.url);

const png = (svg: string, width: number) =>
  new Resvg(svg, {
    fitTo: { mode: 'width', value: width },
    font: { loadSystemFonts: true, defaultFontFamily: 'Helvetica Neue' },
    shapeRendering: 0, // crispEdges: keep pixels square
  })
    .render()
    .asPng();

const esc = (s: string) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

// ---- favicon.svg: fist on a rounded tile, readable at 16px ----
const faviconSvg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 25 25" shape-rendering="crispEdges"><rect width="25" height="25" rx="5" fill="${c.bg}" shape-rendering="geometricPrecision"/>${fistRects(2, 2)}</svg>\n`;

// ---- apple-touch-icon: full-bleed tile (iOS rounds the corners itself) ----
const touchSvg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 30 30" shape-rendering="crispEdges"><rect width="30" height="30" fill="${c.bg}"/>${fistRects(4.5, 5)}</svg>`;

// ---- favicon.ico: single 32×32 PNG entry (valid ICO since Vista) ----
function ico(pngBytes: Uint8Array, size: number): Uint8Array {
  const header = new DataView(new ArrayBuffer(6 + 16));
  header.setUint16(0, 0, true); // reserved
  header.setUint16(2, 1, true); // type: icon
  header.setUint16(4, 1, true); // image count
  header.setUint8(6, size); // width
  header.setUint8(7, size); // height
  header.setUint8(8, 0); // palette
  header.setUint8(9, 0); // reserved
  header.setUint16(10, 1, true); // color planes
  header.setUint16(12, 32, true); // bpp
  header.setUint32(14, pngBytes.length, true); // data size
  header.setUint32(18, 22, true); // data offset
  const file = new Uint8Array(22 + pngBytes.length);
  file.set(new Uint8Array(header.buffer), 0);
  file.set(pngBytes, 22);
  return file;
}

// ---- og.png: 1200×630 ----
function ogSvg(): string {
  const W = 1200;
  const H = 630;
  const cell = 13;
  const fx = 905 - (FIST_W * cell) / 2;
  const fy = 330 - (FIST_H * cell) / 2;

  const burst = Array.from({ length: 24 }, (_, i) => {
    const r = i % 2 === 0 ? 245 : 150;
    const a = (Math.PI * 2 * i) / 24 - Math.PI / 2 + 0.12;
    return `${(905 + r * Math.cos(a)).toFixed(1)},${(325 + r * Math.sin(a)).toFixed(1)}`;
  }).join(' ');

  const lines = Array.from({ length: 18 }, (_, i) => {
    const a = (Math.PI * 2 * i) / 18 + 0.05;
    const r1 = 262 + (i % 3) * 14;
    const r2 = 330;
    return `<line x1="${905 + r1 * Math.cos(a)}" y1="${325 + r1 * Math.sin(a)}" x2="${905 + r2 * Math.cos(a)}" y2="${325 + r2 * Math.sin(a)}"/>`;
  }).join('');

  const [a, b] = og.nameParts;
  const sans = `font-family="Helvetica Neue, Arial" font-weight="700"`;
  const mono = `font-family="Menlo, monospace"`;
  const promise = og.promise
    .map(
      (line, i) =>
        `<text x="80" y="${452 + i * 46}" ${sans} font-size="36" fill="${i === 1 ? c.volt : c.text}">${esc(line)}</text>`,
    )
    .join('');

  return `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}">
  <defs>
    <radialGradient id="glow" cx="0.76" cy="0.52" r="0.55">
      <stop offset="0" stop-color="${c.punch}" stop-opacity="0.38"/>
      <stop offset="1" stop-color="${c.punch}" stop-opacity="0"/>
    </radialGradient>
    <pattern id="scan" width="4" height="4" patternUnits="userSpaceOnUse">
      <rect width="4" height="1" fill="#000" fill-opacity="0.25"/>
    </pattern>
  </defs>
  <rect width="${W}" height="${H}" fill="${c.bg}"/>
  <rect width="${W}" height="${H}" fill="url(#glow)"/>

  <g stroke="${c.text}" stroke-opacity="0.35" stroke-width="6" stroke-linecap="square">${lines}</g>
  <polygon points="${burst}" fill="${c.volt}" fill-opacity="0.92"/>
  <g transform="translate(${fx + 8} ${fy + 10}) scale(${cell})" opacity="0.9">${fistRects().replace(/fill="[^"]+"/g, `fill="${c.ink}"`)}</g>
  <g transform="translate(${fx} ${fy}) scale(${cell})" shape-rendering="crispEdges">${fistRects()}</g>
  <g transform="translate(1040 140) rotate(8)">
    <rect x="-8" y="-8" width="128" height="88" fill="${c.punch}"/>
    <rect x="-14" y="-14" width="128" height="88" fill="${c.text}" stroke="${c.ink}" stroke-width="5"/>
    <text x="50" y="52" text-anchor="middle" font-family="Hiragino Sans" font-weight="900" font-size="58" fill="${c.ink}">${esc('拳!')}</text>
  </g>

  <rect width="${W}" height="${H}" fill="url(#scan)"/>

  <text x="80" y="112" ${mono} font-size="22" letter-spacing="2" fill="${c.volt}">${esc(og.status)}</text>

  <g font-family="Helvetica Neue, Arial" font-weight="700" font-size="176" letter-spacing="-8">
    <text x="88" y="284" fill="${c.punchLo}">${esc(a)}<tspan fill="${c.ink}">${esc(b)}</tspan></text>
    <text x="80" y="276" fill="${c.text}">${esc(a)}<tspan fill="${c.punch}">${esc(b)}</tspan></text>
  </g>

  <g transform="translate(80 350)">
    <rect width="86" height="44" rx="7" fill="${c.ado}"/>
    <text x="43" y="32" text-anchor="middle" ${mono} font-weight="700" font-size="26" fill="${c.ink}">${esc(og.wordplay.left)}</text>
    <text x="112" y="33" ${sans} font-size="30" fill="${c.muted}">+</text>
    <text x="148" y="38" font-family="Hiragino Sans" font-weight="700" font-size="44" fill="${c.punch}">${esc(og.wordplay.right)}</text>
    <text x="204" y="32" ${mono} font-size="22" fill="${c.muted}">${esc(`${og.wordplay.rightReading} · ${og.wordplay.rightMeaning}`)}</text>
  </g>

  ${promise}

  <text x="80" y="${H - 34}" ${mono} font-size="22" fill="${c.muted}">${esc(new URL(meta.url).host)}  ·  follow ${esc(x.handle)} on X</text>
</svg>`;
}

const icon32 = png(faviconSvg, 32);
await Promise.all([
  Bun.write(new URL('favicon.svg', out), faviconSvg),
  Bun.write(new URL('favicon.ico', out), ico(icon32, 32)),
  Bun.write(new URL('apple-touch-icon.png', out), png(touchSvg, 180)),
  Bun.write(new URL('og.png', out), png(ogSvg(), 1200)),
]);
console.log('wrote static/{favicon.svg,favicon.ico,apple-touch-icon.png,og.png}');
