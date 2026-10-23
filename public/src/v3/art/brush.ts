/**
 * Original brush-lettered 拳 (ken, "fist"), built at render time from ten hand-placed stroke
 * centerlines in stroke order. Each stroke is filled as a tapered outline (the "brush"), and its
 * centerline doubles as the reveal mask for the draw-in animation. Output is plain SVG path data.
 */

type Pt = [number, number];
/** Cubic Bézier segments; a line is a cubic with controls on the chord. */
type Seg = [Pt, Pt, Pt, Pt];
type Kind = 'dot' | 'bar' | 'sweep' | 'flare' | 'hook' | 'enso';

interface StrokeDef {
  kind: Kind;
  segs: Seg[];
  /** Base width in viewBox units (the box is 1000 × 1000). */
  w: number;
}

const STROKES: StrokeDef[] = [
  // 龹: two dots, two bars, the wide 人
  { kind: 'dot', w: 88, segs: [[[318, 64], [342, 92], [366, 136], [382, 188]]] },
  { kind: 'dot', w: 84, segs: [[[690, 58], [676, 102], [650, 146], [612, 186]]] },
  { kind: 'bar', w: 62, segs: [[[238, 262], [400, 252], [600, 244], [760, 240]]] },
  { kind: 'bar', w: 66, segs: [[[146, 384], [360, 372], [640, 360], [858, 358]]] },
  { kind: 'sweep', w: 84, segs: [[[508, 92], [506, 300], [420, 470], [96, 604]]] },
  { kind: 'flare', w: 78, segs: [[[540, 392], [626, 478], [740, 552], [930, 594]]] },
  // 手: the flat top sweep, two bars, the hooked spine
  { kind: 'sweep', w: 62, segs: [[[664, 604], [592, 632], [480, 652], [330, 668]]] },
  { kind: 'bar', w: 58, segs: [[[276, 752], [420, 746], [580, 740], [724, 736]]] },
  { kind: 'bar', w: 64, segs: [[[176, 862], [380, 852], [620, 842], [830, 840]]] },
  {
    kind: 'hook',
    w: 78,
    segs: [[[500, 650], [501, 740], [503, 830], [504, 912]], [[504, 912], [506, 958], [488, 976], [420, 940]]],
  },
];

function bez([a, b, c, d]: Seg, t: number): Pt {
  const u = 1 - t;
  return [
    u * u * u * a[0] + 3 * u * u * t * b[0] + 3 * u * t * t * c[0] + t * t * t * d[0],
    u * u * u * a[1] + 3 * u * u * t * b[1] + 3 * u * t * t * c[1] + t * t * t * d[1],
  ];
}

/** The stroke flattened to a dense polyline, with the cumulative arc length at each point. */
function densify(segs: Seg[]): { dense: Pt[]; acc: number[] } {
  const dense: Pt[] = segs.flatMap((s, i) => Array.from({ length: 120 }, (_, k) => bez(s, (k + (i ? 1 : 0)) / 120)));
  dense.push(segs.at(-1)![3]);
  const acc = [0];
  for (let i = 1; i < dense.length; i++) acc.push(acc[i - 1] + Math.hypot(dense[i][0] - dense[i - 1][0], dense[i][1] - dense[i - 1][1]));
  return { dense, acc };
}

/** Evenly spaced points along the whole stroke, by arc length. */
function sample(segs: Seg[], n: number): Pt[] {
  const { dense, acc } = densify(segs);
  const total = acc.at(-1)!;
  const out: Pt[] = [];
  let j = 0;
  for (let k = 0; k <= n; k++) {
    const target = (total * k) / n;
    while (j < acc.length - 2 && acc[j + 1] < target) j++;
    const f = (target - acc[j]) / (acc[j + 1] - acc[j] || 1);
    out.push([dense[j][0] + (dense[j + 1][0] - dense[j][0]) * f, dense[j][1] + (dense[j + 1][1] - dense[j][1]) * f]);
  }
  return out;
}

const bump = (t: number, at: number, spread: number) => Math.exp(-(((t - at) / spread) ** 2));

/** Width multiplier along the stroke, 0 → 1. */
function profile(kind: Kind, t: number): number {
  switch (kind) {
    case 'dot':
      return 0.34 + 0.8 * Math.sin(Math.min(t * 1.25, 1) * (Math.PI / 2)) ** 1.4;
    case 'bar':
      return 0.86 + 0.42 * bump(t, 0.03, 0.07) - 0.12 * bump(t, 0.5, 0.3) + 0.5 * bump(t, 0.95, 0.06);
    case 'sweep':
      return (1.08 + 0.3 * bump(t, 0.03, 0.06)) * (1 - t) ** 1.15 + 0.03;
    case 'flare':
      return t < 0.78 ? 0.4 + 0.8 * (t / 0.78) ** 1.3 : 1.2 * (1 - (t - 0.78) / 0.22) ** 0.8 + 0.03;
    case 'hook':
      return t < 0.74 ? 0.92 + 0.35 * bump(t, 0.02, 0.05) : (1.02 + 0.25 * bump(t, 0.76, 0.03)) * (1 - (t - 0.74) / 0.26) ** 1.1 + 0.03;
    case 'enso':
      // pressed hard at the start, a long even body, then the brush lifts into dry tail
      return (0.75 + 0.55 * bump(t, 0.04, 0.08) + 0.15 * Math.sin(t * 9)) * (1 - t ** 3.2) + 0.04;
  }
}

const r = (v: number) => Math.round(v * 10) / 10;

function outline(def: StrokeDef): string {
  const pts = sample(def.segs, 64);
  const left: Pt[] = [];
  const right: Pt[] = [];
  const widths = pts.map((_, i) => (def.w * profile(def.kind, i / (pts.length - 1))) / 2);
  pts.forEach((p, i) => {
    const a = pts[Math.max(0, i - 1)];
    const b = pts[Math.min(pts.length - 1, i + 1)];
    const len = Math.hypot(b[0] - a[0], b[1] - a[1]) || 1;
    const nx = -(b[1] - a[1]) / len;
    const ny = (b[0] - a[0]) / len;
    left.push([p[0] + nx * widths[i], p[1] + ny * widths[i]]);
    right.push([p[0] - nx * widths[i], p[1] - ny * widths[i]]);
  });
  const w0 = widths[0];
  const w1 = widths.at(-1)!;
  const rr = right.reverse();
  return [
    `M${r(left[0][0])} ${r(left[0][1])}`,
    ...left.slice(1).map(([x, y]) => `L${r(x)} ${r(y)}`),
    `A${r(w1)} ${r(w1)} 0 0 0 ${r(rr[0][0])} ${r(rr[0][1])}`,
    ...rr.slice(1).map(([x, y]) => `L${r(x)} ${r(y)}`),
    `A${r(w0)} ${r(w0)} 0 0 0 ${r(left[0][0])} ${r(left[0][1])}Z`,
  ].join('');
}

function centerline(segs: Seg[]): string {
  const [[x, y]] = segs[0];
  return `M${x} ${y}` + segs.map(([, b, c, d]) => `C${b[0]} ${b[1]} ${c[0]} ${c[1]} ${d[0]} ${d[1]}`).join('');
}

export interface BrushStroke {
  /** Filled brush shape. */
  fill: string;
  /** Centerline, used as the reveal mask. */
  path: string;
  /** Real length of the centerline in viewBox units (no `pathLength` normalization). */
  length: number;
  /** Mask stroke width: wide enough to cover the fattest part of the brush. */
  maskWidth: number;
  /** Animation delay and duration, seconds. */
  delay: number;
  duration: number;
}

/**
 * Inline custom properties for a mask path's draw-in, all in user units: dash = the path,
 * gap and start offset leave room for the round caps so nothing shows before the stroke starts.
 * Only the animation's keyframes use them; at rest the mask has no dash at all.
 */
export function drawVars(s: BrushStroke): string {
  const len = Math.ceil(s.length);
  return `--len:${len}px;--gap:${len + 2 * s.maskWidth}px;--off:${len + s.maskWidth}px;--d:${s.delay}s;--t:${s.duration}s`;
}

/** 拳 in stroke order, with draw-in timing (starts at `start`, total ≈ 3.4 s). */
export function kenStrokes(start = 0.2): BrushStroke[] {
  let t = start;
  return STROKES.map((def) => {
    const duration = def.kind === 'dot' ? 0.22 : def.kind === 'bar' ? 0.3 : 0.42;
    const stroke = {
      fill: outline(def),
      path: centerline(def.segs),
      length: densify(def.segs).acc.at(-1)!,
      maskWidth: Math.round(def.w * 1.9),
      delay: Math.round(t * 100) / 100,
      duration,
    };
    t += duration + 0.08;
    return stroke;
  });
}

/** Ink droplets flicked off the sweeps: [cx, cy, r]. */
export const splatter: [number, number, number][] = [
  [70, 628, 9], [52, 650, 4], [956, 612, 7], [978, 598, 3.5], [382, 214, 5], [430, 990, 6], [405, 1004, 3],
  [880, 368, 4], [118, 402, 3],
];

/**
 * An ensō (円相), the one-stroke brush circle, around a 1000 × 1000 box. Built from cubic arcs
 * with a slight wobble in the radius so it reads as hand-drawn; it stops short of closing.
 */
export function ensoStroke(): BrushStroke {
  const cx = 500;
  const cy = 505;
  const start = (-115 * Math.PI) / 180;
  const sweep = (322 * Math.PI) / 180;
  const n = 6;
  const step = sweep / n;
  // Handle length for a cubic that approximates a circular arc of `step` radians.
  const k = (4 / 3) * Math.tan(step / 4);
  const radius = [392, 404, 398, 386, 380, 392, 408];
  const at = (i: number): Pt => {
    const a = start + i * step;
    return [cx + radius[i] * Math.cos(a), cy + radius[i] * Math.sin(a)];
  };
  const segs: Seg[] = Array.from({ length: n }, (_, i) => {
    const a0 = start + i * step;
    const a1 = a0 + step;
    const [p0, p3] = [at(i), at(i + 1)];
    return [
      p0,
      [p0[0] - k * radius[i] * Math.sin(a0), p0[1] + k * radius[i] * Math.cos(a0)],
      [p3[0] + k * radius[i + 1] * Math.sin(a1), p3[1] - k * radius[i + 1] * Math.cos(a1)],
      p3,
    ];
  });
  const def: StrokeDef = { kind: 'enso', w: 96, segs };
  return { fill: outline(def), path: centerline(segs), length: densify(segs).acc.at(-1)!, maskWidth: 200, delay: 0, duration: 1.4 };
}
