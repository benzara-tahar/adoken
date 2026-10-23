# adoken image prompts

Reusable prompt templates for social images, banners, OG/link cards, profile assets, video thumbnails, and the anime/arcade background showcases. Each template has one tool-neutral brief and three tuned variants: **Midjourney**, **Flux**, **GPT Image**.

Brand tokens come from `public/src/theme.ts` (site) and `crates/theme` (app). If those change, update §1 first.

Tool notes checked 2026-09-25:

- **Midjourney**: default model V8.2. Parameters go at the end, space before `--`, no punctuation inside parameters. `--ar` takes integers; `--raw`, `--stylize`, `--no`, `--hd` (≈2K, caps `--ar` at 4:1) are available on V8.x ([Version docs](https://docs.midjourney.com/hc/en-us/articles/32199405667853-Version), [Parameter list](https://docs.midjourney.com/hc/en-us/articles/32859204029709-Parameter-List)). For anime plates, try the current Niji model (`--niji`, check the docs for the version number).
- **Flux (FLUX.2 [pro]/[max])**: no negative prompts. Say what you want ("empty scene", "sharp focus throughout"), because negated words tend to appear anyway. Tie hex codes to objects ("the fist is hex #ff5a36"). Quoted text of 1–4 words renders best. 30–80 words is the sweet spot; JSON-structured prompts are supported ([BFL prompting guide](https://docs.bfl.ml/guides/prompting_guide_flux2)).
- **GPT Image** (`gpt-image-2.5-sunburst` for detail, `gpt-image-2.5-flare` for fast drafts): arbitrary `WIDTHxHEIGHT` if both are divisible by 16 and the ratio is between 1:3 and 3:1; above 2560x1440 is experimental. `background: "transparent"` with `output_format: "png"` for stickers ([Images API](https://developers.openai.com/api/reference/resources/images/methods/generate)).

---

## 0. Ground rules (read before generating anything)

1. **Never AI-generate product UI.** Every adoken window, diff, inbox, or Cmd+P panel in an image is a **real screenshot from the demo org**, composited on top of a generated plate. Generated "fake UI" looks wrong to devs instantly and breaks trust.
2. **Never AI-generate the logo.** The pixel fist is code: `public/src/art/fist.ts` (21×20 cells). Export it as SVG/PNG and composite it. Prompts below describe it only so the model leaves the right space or matches its style.
3. **Real copy goes on in post.** Headlines, numbers, and labels are set in Figma/Keynote/Pixelmator with the brand fonts (§1.3). Models may render at most 1–3 display words in quotes ("K.O.", "拳"), and only when the template says so.
4. **No Capcom / Street Fighter IP, and no fighting-game tropes.** Not in prompts, not in outputs. The full banned list lives in `docs/design/ken.md` §8 (names, moves, catchphrases, visuals); the short version: no Hadouken/Shoryuken or palms-forward energy-ball poses, no rising-uppercut poses, no Ryu/Ken Masters or any fighter, no martial-arts gi or headband, no character portraits with health bars, no VS splash screens, no fighting-game HUD layouts (health bars, round timers), no announcer lines ("Round 1", "Fight!", "Perfect", "You Win"). Our fighter is the **fist alone**, no body. Our projectile-flavored motif is the **Merge Wave** (波), drawn as ink-wash brush strokes.
5. **No Microsoft marks.** No Azure/ADO logos or product icons, no Microsoft colors used as a brand reference. In comparisons, crop the ADO logo out of real screenshots.
6. **No real people, no artist names.** Don't prompt "in the style of" any living artist or studio. Describe the look instead (§1.4).
7. **Check before posting:** zoom to 100% for garbled text, extra fingers on the fist, hidden logos, anything that resembles a known character. When in doubt, regenerate.
8. **Log every keeper** in §5 (template, tool, model, seed, prompt, file). Reuse seeds for consistent series.

---

## 1. Brand style guide

### 1.1 Palette

| Token | Hex | Role in images |
| --- | --- | --- |
| `bg` | `#0b0b10` | Canvas. Near-black with a hint of indigo. Most images are 70%+ this. |
| `surface` | `#14141d` | Panels, cards, window chrome behind screenshots |
| `line` | `#2a2a3d` | Grids, dividers, scanlines |
| `text` | `#f4f1ea` | Warm off-white for all type |
| `muted` | `#aaa6bc` | Secondary type, captions |
| `punch` | `#ff5a36` | **Hero accent.** The fist, key numbers, one focal element per image |
| `punchHi` / `punchLo` | `#ffb199` / `#b8321c` | Fist highlight / shade, glow falloff |
| `ink` | `#1a0d12` | Pixel outlines, text on volt/punch |
| `volt` | `#ffd23f` | Impact bursts, the 拳 burst, wrist wrap. Sparingly. |
| `voltHi` | `#fff1a8` | Burst core |
| `ado` | `#4cc9ff` | Cool counter-accent: links, "ADO side" of a split, UI highlights |

Ratio rule of thumb: 75% `bg`/`surface`, 15% `text`/`muted`, 7% `punch`, 3% `volt` + `ado`.

### 1.2 Composition

- Dark-first, high contrast, one focal point. Left-to-right read: context → punch.
- Keep a clean **safe zone** for type (usually left 45%).
- Motion comes from speed lines and impact frames, not blur.
- Subtle CRT scanlines (2–4% opacity) are allowed on arcade plates, never over screenshots.
- Pixel art is crisp: nearest-neighbor scaling only, integer multiples.

### 1.3 Typography (applied in post, never generated)

| Use | Font | Notes |
| --- | --- | --- |
| Headlines, numbers, labels | `ui-monospace` / SF Mono (Menlo fallback), 800 weight, tight tracking (−0.04em) | Matches adoken.dev headings |
| Body / captions | System sans (SF Pro / Segoe UI) | |
| 拳 | Hiragino Sans (Noto Sans JP fallback), 900 weight | Inside a volt burst with ink text |
| Eyebrows | Mono, uppercase, +0.14em tracking, `muted` | e.g. "STAGE 3 · SYNC" |

### 1.4 Motifs

- **The fist** (hero): chunky 8-bit pixel-art fist seen from the front, like a boxing glove: `punch` orange body, `punchHi` highlights, `punchLo` shading, `volt` yellow wrist wrap, thick `ink` outline. No arm, no body, no character.
- **拳 burst:** the kanji in a jagged comic-style starburst, `volt` fill, `ink` text.
- **Impact frame:** radial speed lines converging on the focal point, halftone dots in `punchLo`.
- **Arcade motifs (ours):** a hi-score table for perf numbers; a combo counter ("×7"); "STAGE CLEAR" and an 8-tile stage strip for milestones; "K.O." for merges; "PRESS ⌘P TO START" in mono; the Merge Wave (波) as ink-wash strokes. Never health bars or VS screens (see ground rule 4). The weekly **fight card** export from the app (`docs/design/ken.md` §5, 1200×675 PNG, numbers only by default) is a ready-made real asset: prefer it over any generated stats image.
- **Anime look (for background showcases):** cel-shaded 2D animation background painting, clean line art, soft gradient skies, gentle bloom on lights, painterly clouds, city or nature scenes, no characters, dusk/night palettes that sit well behind a dark UI.

### 1.5 Accessibility

- Text contrast ≥ 4.5:1 (off-white on `bg` passes easily; never `muted` on `surface` below 14 px).
- Every posted image gets alt text that states what's shown and any numbers in it.
- No flashing frames in GIFs/videos (≤ 3 flashes/second); impact frames last ≥ 150 ms.

---

## 2. Size cheat sheet

| Asset | Final size | Midjourney | Flux | GPT Image (generate at) | Then |
| --- | --- | --- | --- | --- | --- |
| X header (H1) | 1500×500 | `--ar 3:1` | 1536×512 | `1536x512` | Resize to 1500×500 |
| Avatar (A1) | 400×400 | `--ar 1:1` | 1024×1024 | `1024x1024` | Resize |
| OG / link card (O1) | 1200×630 | `--ar 40:21` | 1216×640 | `1216x640` | Crop to 1200×630 |
| In-feed card 16:9 (C1–C3, B1) | 1600×900 | `--ar 16:9` | 1792×1008 | `1792x1008` | Resize |
| In-feed card 4:5 (mobile-first) | 1080×1350 | `--ar 4:5` | 1088×1360 | `1088x1360` | Resize |
| Video thumbnail (T1) | 1280×720 | `--ar 16:9` | 1280×720 | `1280x720` | — |
| dev.to cover (D1) | 1000×420 | `--ar 50:21` | 1536×640 | `1536x640` | Resize/crop |
| Wallpaper, MacBook (W1–W3) | 2560×1600+ | `--ar 16:10 --hd` | 2304×1440 | `2304x1440` | Upscale ×1.25–1.5 |
| Sticker (K1) | 512×512, transparent | `--ar 1:1` then remove bg | 1024×1024 | `1024x1024`, `background: transparent` | Trim |

X header safe zone: the avatar covers the lower-left (~ first 300 px from the left, bottom 40%) on desktop, and mobile crops ~60 px top and bottom. Keep focal content in the center-right band.

---

## 3. Templates

Placeholders in `{braces}`. Unfilled defaults are in the brief.

### H1. X profile header (1500×500)

**Brief:** A wide, dark arcade-meets-desk scene. Right third: the pixel fist punching toward the viewer out of a glowing screen edge, with a volt impact burst and speed lines. Center: calm dark space with a faint grid (where "adoken · Azure DevOps, with a punch" is set in post). Left: empty (avatar overlap).

**Midjourney**

```text
wide cinematic banner, near-black indigo background, a chunky 8-bit pixel-art fist seen from the front bursting out of a glowing orange screen edge on the right side, orange fist with yellow wrist wrap and thick dark outline, bright yellow comic impact starburst behind it, radial speed lines, faint dark grid floor, subtle CRT scanlines, large empty dark space in the center and left, crisp pixel art, high contrast, minimal --ar 3:1 --raw --stylize 150 --no text, letters, logo, watermark, character, person, arm, body, gi, headband
```

**Flux**

```text
Wide dark banner. On the right, a chunky 8-bit pixel-art fist, seen from the front, punches out of a glowing screen edge toward the viewer; the fist is hex #ff5a36 orange with hex #ffb199 highlights, a hex #ffd23f yellow wrist wrap and a thick hex #1a0d12 outline. Behind it, a hex #ffd23f comic starburst and radial speed lines. The background is hex #0b0b10 with a faint hex #2a2a3d grid. The center and left are calm, empty dark space. Crisp pixel art, sharp focus throughout.
```

**GPT Image** (`gpt-image-2.5-sunburst`, `size: 1536x512`, `quality: high`)

```text
Create a 3:1 profile header for a developer tool called adoken. Background: near-black indigo (#0b0b10) with a faint dark grid (#2a2a3d) and very subtle CRT scanlines. On the right third only: a chunky 8-bit pixel-art fist seen from the front, bursting out of a glowing screen edge toward the viewer. The fist is orange (#ff5a36) with lighter highlights (#ffb199), darker shading (#b8321c), a yellow wrist wrap (#ffd23f), and a thick dark outline (#1a0d12). Put a yellow comic-style impact starburst and radial speed lines behind it. Keep the center and left 60% empty and dark for text that will be added later. Do not include any text, letters, logos, people, arms, or characters. The fist is disembodied.
```

### A1. Avatar for @adokendev (400×400)

**Brief:** The fist, centered, on a `punch` circle with a thin `volt` ring, `bg` outside. Prefer compositing the real SVG from `fist.ts`; generate only the circular plate.

**Midjourney** (plate only)

```text
flat circular badge, solid warm orange disc with a thin bright yellow outer ring, on a near-black background, subtle pixel dithering at the disc edge, centered, empty center, minimal, crisp --ar 1:1 --raw --stylize 50 --no text, logo, fist, hand, face
```

**Flux** (plate only)

```text
A flat circular badge centered on a square canvas. The disc is solid hex #ff5a36 orange with subtle pixel dithering at its edge and a thin hex #ffd23f yellow ring around it. The canvas outside the ring is hex #0b0b10. The center of the disc is empty and clean. Minimal, crisp, sharp focus throughout.
```

**GPT Image** (`size: 1024x1024`)

```text
A flat circular badge, centered: an orange (#ff5a36) disc with subtle pixel dithering at the edge and a thin yellow (#ffd23f) ring, on a near-black (#0b0b10) square. Leave the center of the disc completely empty; a pixel-art logo will be placed there later. No text, no symbols, no hands.
```

Then: composite `fist.ts` exported at 16× (nearest neighbor), fist ≈ 62% of the disc diameter, optically centered 2% low.

### O1. OG / link card (1200×630) for dev.to posts and shared links

adoken.dev's own OG image is generated in code (`bun run assets` in `public/`). Use O1 only for essays and one-off shares.

**Brief:** Left 55%: empty `bg` for the title (set in post, mono 800). Right: `{scene}`, e.g. the fist punching through a stack of `{object}` (emails, spinners, tabs), impact burst, speed lines.

**Midjourney**

```text
dark editorial illustration, near-black indigo background, on the right side a chunky 8-bit pixel-art orange fist with yellow wrist wrap smashing through {object}, pieces flying apart as pixel shards, yellow impact starburst, radial speed lines, left half empty dark space, crisp pixel art mixed with clean flat vector shapes, high contrast --ar 40:21 --raw --stylize 120 --no text, letters, logo, watermark, person, arm
```

**Flux**

```text
Editorial illustration on a hex #0b0b10 background. On the right, a chunky 8-bit pixel-art fist (hex #ff5a36 with a hex #ffd23f wrist wrap and hex #1a0d12 outline) smashes through {object}, which breaks into flying pixel shards. A hex #ffd23f starburst and radial speed lines mark the impact. The left half is calm empty dark space. Crisp pixel art with clean flat shapes, sharp focus throughout.
```

**GPT Image** (`size: 1216x640`)

```text
Wide editorial illustration, dark background (#0b0b10). Right half: a chunky 8-bit pixel-art fist (orange #ff5a36, yellow wrist wrap #ffd23f, dark outline #1a0d12) smashing through {object}, which shatters into flying pixel shards, with a yellow comic impact starburst and radial speed lines. Left half: empty and dark, reserved for a headline added later. No text, logos, people, or arms.
```

`{object}` ideas: `a tall stack of grey envelope icons` (P02), `a wall of browser loading spinners` (P22), `a tangle of browser tabs` (general), `a giant padlock with too many keys` (P37), `an hourglass` (P29).

### C1. Pain-point card, "boss fight" (1600×900 or 1080×1350)

**Brief:** The pain as a pixel-art "boss" object (never a person, never a fighter) looming over a small fist. No HUD: the pain's name is set in post as a mono eyebrow ("BOSS · {boss_label}"). Boss is desaturated grey/blue; the fist is the only warm color.

`{boss}` ideas per pain: `a towering pile of grey envelopes with a red unread badge` (P01–P04), `a giant spinning loading wheel` (P22), `a massive stopwatch with a cobweb` (P29), `an enormous key ring with forty identical keys` (P37), `a huge zip archive with a magnifying glass` (P31).

**Midjourney**

```text
retro arcade boss battle scene in 16-bit pixel art, dark indigo arena, on the right a giant {boss} in desaturated grey and cold blue looming over the arena, on the left a small chunky orange pixel fist with yellow wrist wrap ready to punch, empty dark band along the top of the frame, subtle scanlines, dramatic rim light, high contrast, no characters --ar 16:9 --raw --stylize 200 --no text, letters, numbers, logo, person, body, arm, timer, health bar
```

**Flux**

```text
A 16-bit pixel-art arcade boss battle on a dark hex #0b0b10 arena. On the right looms a giant {boss} in desaturated grey and cold hex #4cc9ff blue. On the left, a small chunky pixel fist in hex #ff5a36 with a hex #ffd23f wrist wrap, ready to punch. The top edge is a clean empty dark band. Subtle scanlines, dramatic rim light, crisp pixels, sharp focus throughout.
```

**GPT Image** (`size: 1792x1008` or `1088x1360`)

```text
16-bit pixel-art arcade boss battle. Dark arena (#0b0b10). Right side: a giant {boss}, desaturated grey with cold blue (#4cc9ff) light, looming. Left side: a small chunky pixel-art fist, orange (#ff5a36) with a yellow wrist wrap (#ffd23f) and dark outline, ready to punch. Keep a clean empty dark band along the top edge for a label added later. Subtle scanlines. No text, numbers, timers, health bars, logos, people, bodies, or arms.
```

### C2. Hi-score / numbers card (1600×900)

Numbers must be exact, so **generate only the plate**, then set the table in post (mono, `text` on `surface`, the best number in `punch`).

**Midjourney**

```text
empty retro arcade high score screen, dark indigo CRT monitor glass with subtle curvature and scanlines, faint pixel border frame in orange and yellow, large empty central panel, soft glow, crisp, minimal --ar 16:9 --raw --stylize 80 --no text, letters, numbers, logo
```

**Flux**

```text
An empty retro arcade high-score screen: dark hex #0b0b10 CRT glass with gentle curvature and faint scanlines, a thin pixel border frame in hex #ff5a36 with hex #ffd23f corner accents, and a large clean central panel in hex #14141d. Soft glow at the edges, empty scene, sharp focus throughout.
```

**GPT Image** (`size: 1792x1008`)

```text
An empty retro arcade high-score screen. Dark CRT glass (#0b0b10) with slight curvature and faint scanlines, a thin pixel-art border in orange (#ff5a36) with yellow (#ffd23f) corner accents, and a large empty central panel (#14141d) for a table added later. No text or numbers anywhere.
```

Post layout: eyebrow "HI-SCORE TARGETS" or "FRAME DATA · W{n}"; rows `{metric} … {value}`; footnote with dataset, hardware, commit in `muted`.

### C3. Stage clear / milestone card (1600×900)

**Brief:** "STAGE CLEAR" moment for a version (v0.0.1 … v0.1.0). Pixel fist raised upward (a victory punch), confetti of pixel squares in `punch`/`volt`/`ado`, a stage-select strip along the bottom with `{n}` of 8 tiles lit.

**Midjourney**

```text
16-bit pixel art victory screen, dark indigo background, a chunky orange pixel fist with yellow wrist wrap punching straight up, burst of pixel confetti in orange yellow and sky blue, row of eight small square tiles along the bottom with the first {n} glowing, center top empty for a title, crisp pixels, celebratory, clean --ar 16:9 --raw --stylize 150 --no text, letters, numbers, logo, person, arm
```

**Flux**

```text
A 16-bit pixel-art victory screen on hex #0b0b10. A chunky pixel fist in hex #ff5a36 with a hex #ffd23f wrist wrap punches straight up, surrounded by pixel confetti in hex #ff5a36, hex #ffd23f and hex #4cc9ff. Along the bottom, a row of eight small square tiles; the first {n} glow hex #ffd23f, the rest are dim hex #2a2a3d. The top center is empty dark space. Crisp, celebratory, sharp focus throughout.
```

**GPT Image** (`size: 1792x1008`)

```text
16-bit pixel-art victory screen. Dark background (#0b0b10). A chunky pixel-art fist (orange #ff5a36, yellow wrist wrap #ffd23f, dark outline) punches straight up, surrounded by pixel confetti in orange, yellow, and sky blue (#4cc9ff). Along the bottom: a row of exactly eight small square tiles; the first {n} glow yellow, the rest are dim (#2a2a3d). Leave the top center empty for a title added later. No text, numbers, logos, people, or arms.
```

Post text on the card: `STAGE CLEAR` (mono 800, `text`), `v0.0.{n}` in `punch`, one-line summary in `muted`.

### B1. Before/after comparison frame (1600×900)

**Honesty rules:** both sides are **real recordings or screenshots** of the same task on the same machine and network, using the demo org. Never speed up, slow down, or cherry-pick one side. Crop the Azure DevOps logo from the "before" side and label it "ADO web · demo org". Timers are real (screen-recorded stopwatch or in-app timing).

**Brief (plate only):** Split frame, left half cool and desaturated (`ado` tint at 10%), right half warm (`punch` rim light), a jagged vertical impact seam in `volt` down the middle, two empty rounded windows for screenshots.

**Midjourney**

```text
split screen composition, left half cool desaturated blue grey, right half warm dark with orange rim light, a jagged bright yellow lightning seam dividing the halves vertically, two empty dark rounded rectangles one on each side for screenshots, subtle pixel dithering on the seam, dark indigo base, clean, minimal --ar 16:9 --raw --stylize 60 --no text, letters, logo, UI, interface, people
```

**Flux**

```text
A split-screen plate on hex #0b0b10. The left half is cool and desaturated with a faint hex #4cc9ff tint; the right half is warm with a hex #ff5a36 rim light. A jagged hex #ffd23f lightning seam divides them vertically. Each half holds one empty dark hex #14141d rounded rectangle, ready for a screenshot. Clean, minimal, empty scene, sharp focus throughout.
```

**GPT Image** (`size: 1792x1008`)

```text
A split-screen background plate. Base color #0b0b10. Left half: cool, desaturated, faint blue tint (#4cc9ff at low strength). Right half: warm, with an orange (#ff5a36) rim light. A jagged yellow (#ffd23f) lightning seam divides them vertically. On each side, one empty dark (#14141d) rounded rectangle where a screenshot will be placed. No text, logos, UI, or people.
```

Post: labels "Before · ADO web" / "After · adoken" in mono; timer values in `punch` only on the adoken side if faster, and only if measured.

### W1. Anime background showcase: city dusk (2560×1600)

For demoing custom backgrounds. Generated wallpapers are ours to use; still log them. The app shows backgrounds with blur/dim, so compose for that: detail on the edges and sky, calm and darker where the window sits (center), no faces.

**Midjourney**

```text
anime background art, cel-shaded 2D animation background painting, a quiet hillside overlooking a coastal city at dusk, violet and orange gradient sky with soft painterly clouds, first city lights glowing, power lines and a vending machine in the foreground, gentle bloom, calm darker center, no characters, highly detailed, cinematic --ar 16:10 --hd --stylize 250 --no text, people, character, face, logo, signage letters
```

(Anime-tuned alternative: same prompt with `--niji`, current version per docs.)

**Flux**

```text
Cel-shaded anime background painting: a quiet hillside overlooking a coastal city at dusk. The sky is a gradient from violet to hex #ff5a36 orange with soft painterly clouds; the first city lights glow warm hex #ffd23f. Power lines and a glowing vending machine frame the foreground edges. The center is calm and slightly darker. An empty scene with no people, gentle bloom, cinematic, sharp focus throughout.
```

**GPT Image** (`size: 2304x1440`)

```text
An anime-style background painting (cel-shaded 2D animation background, clean line art, painterly clouds): a quiet hillside overlooking a coastal city at dusk. Violet-to-orange gradient sky, first city lights glowing, power lines and a glowing vending machine at the foreground edges. Keep the center calm and a bit darker so a translucent app window can sit on top. No people or characters, no readable text or signage, no logos.
```

Variants (swap the scene sentence, keep everything else): `W1a` rainy rooftop at night with neon reflections in puddles; `W1b` snowy mountain train station at blue hour; `W1c` summer riverbank at sunset with fireflies; `W1d` late-night convenience store street with warm window light.

Showcase recipe: real adoken screenshot over the wallpaper at the app's default dim/blur, plus a 3-frame GIF cycling W1 → W1a → W2 to show switching. Check legibility at 40% dim + 20 px blur.

### W2. Pixel arcade wallpaper (2560×1600)

**Midjourney**

```text
16-bit pixel art wallpaper, night arcade street with glowing cabinet silhouettes at the far left and right edges, dark indigo sky with pixel stars, wet pavement reflecting orange and yellow neon, calm dark empty center, crisp pixels, atmospheric, no characters --ar 16:10 --hd --raw --stylize 180 --no text, letters, logo, people, character
```

**Flux**

```text
A 16-bit pixel-art night street lined with glowing arcade cabinet silhouettes at the far left and right edges. Dark hex #0b0b10 sky with small pixel stars. Wet pavement reflects hex #ff5a36 and hex #ffd23f neon. The center is calm, dark, and empty. Crisp pixels, atmospheric, empty scene, sharp focus throughout.
```

**GPT Image** (`size: 2304x1440`)

```text
16-bit pixel-art wallpaper: a night street with glowing arcade cabinet silhouettes only at the far left and right edges, a dark indigo sky (#0b0b10) with small pixel stars, and wet pavement reflecting orange (#ff5a36) and yellow (#ffd23f) neon. Keep the center dark, calm, and empty. No text, readable signs, logos, people, or characters.
```

### W3. "Plain dark, thanks" wallpaper (2560×1600)

For the people who don't want anime. A subtle `bg` gradient with a faint pixel-grid vignette.

**Flux**

```text
A minimal dark wallpaper: a smooth gradient from hex #14141d at the top to hex #0b0b10 at the bottom, with a very faint hex #2a2a3d pixel grid fading out toward the edges and a barely visible warm hex #ff5a36 glow low in one corner. Clean, calm, empty scene, sharp focus throughout.
```

(Midjourney/GPT Image: same sentence; add `--ar 16:10 --raw --stylize 30 --no text, logo` for Midjourney, `size: 2304x1440` for GPT Image.)

### T1. Demo video thumbnail (1280×720)

**Brief:** Right 55%: a real frame from the video (the payoff moment). Left 45%: `bg` panel with 2–4 words in mono 800 (set in post), e.g. "CMD+P · 100K ITEMS". Generate only the left accent plate: fist in motion entering from the left edge with speed lines.

**Midjourney**

```text
dark indigo panel, chunky orange 8-bit pixel fist with yellow wrist wrap entering from the left edge in motion, horizontal speed lines, small yellow impact burst at the right edge, lots of empty dark space in the middle, crisp pixel art --ar 16:9 --raw --stylize 100 --no text, letters, logo, person, arm
```

**Flux**

```text
A dark hex #0b0b10 panel. A chunky 8-bit pixel fist in hex #ff5a36 with a hex #ffd23f wrist wrap enters from the left edge in fast motion, trailing horizontal speed lines, with a small hex #ffd23f impact burst at the far right edge. The middle is wide empty dark space. Crisp pixel art, sharp focus throughout.
```

**GPT Image** (`size: 1280x720`)

```text
A dark panel (#0b0b10). A chunky 8-bit pixel-art fist (orange #ff5a36, yellow wrist wrap #ffd23f, dark outline) enters from the left edge in fast motion with horizontal speed lines; a small yellow impact burst sits at the far right edge. The middle is empty dark space for text added later. No text, logos, people, or arms.
```

### D1. dev.to cover (1000×420)

Use O1 with `--ar 50:21` / `1536x640`, and pick `{object}` from the essay's topic: `a tangle of network cables being punched into a neat single cable` (sync), `a stack of 100,000 tiny index cards` (search), `a film strip of 60 frames` (rendering).

### L1. Launch banner, v0.1.0 (1600×900 and 1500×500)

**Brief:** Title-screen moment: the fist centered, 拳 burst behind it, "PRESS ⌘P TO START" line space below (set in post), stage-select strip with all 8 tiles lit.

**Midjourney**

```text
retro arcade title screen in 16-bit pixel art, dark indigo background with subtle scanlines, a large chunky orange pixel fist with yellow wrist wrap centered and punching toward the viewer, a big bright yellow jagged comic starburst behind it, a row of eight glowing yellow square tiles at the bottom, empty band below the fist for a title, triumphant, crisp --ar 16:9 --raw --stylize 200 --no text, letters, numbers, logo, person, arm
```

**Flux**

```text
A 16-bit pixel-art arcade title screen on hex #0b0b10 with faint scanlines. Centered: a large chunky pixel fist in hex #ff5a36 with a hex #ffd23f wrist wrap, punching toward the viewer, in front of a big jagged hex #ffd23f starburst with a hex #fff1a8 core. Along the bottom, a row of eight glowing hex #ffd23f square tiles. An empty dark band sits below the fist. Triumphant, crisp, sharp focus throughout.
```

**GPT Image** (`size: 1792x1008`, and `1536x512` for the header crop)

```text
16-bit pixel-art arcade title screen. Dark background (#0b0b10), faint scanlines. Centered: a large chunky pixel-art fist (orange #ff5a36, yellow wrist wrap #ffd23f, dark outline) punching toward the viewer, in front of a big jagged yellow starburst (#ffd23f with a pale #fff1a8 core). Bottom: a row of exactly eight glowing yellow square tiles. Leave an empty dark band below the fist for a title added later. No text, numbers, logos, people, or arms.
```

In post: add the 拳 glyph inside the burst (Hiragino 900, `ink`), "adoken v0.1" in mono, "PRESS ⌘P TO START".

### K1. Reaction stickers (transparent PNG)

For replies and GIF-less celebrations. GPT Image handles transparency natively; for Midjourney/Flux, remove the background afterwards.

**GPT Image** (`size: 1024x1024`, `background: "transparent"`, `output_format: "png"`)

```text
A pixel-art sticker on a transparent background: the letters "K.O." in chunky 8-bit block letters, orange (#ff5a36) with a thick dark outline (#1a0d12) and a small yellow (#ffd23f) impact starburst behind them. Crisp pixels, no other text, no characters.
```

**Flux**

```text
A pixel-art sticker on a plain flat white background: the text "K.O." in chunky 8-bit block letters, hex #ff5a36 with a thick hex #1a0d12 outline, over a small hex #ffd23f impact starburst. Crisp pixels, empty scene around the sticker, sharp focus throughout.
```

**Midjourney**

```text
pixel art sticker, the letters "K.O." in chunky 8-bit block letters, orange with thick dark outline, small yellow impact starburst behind, plain white background, crisp --ar 1:1 --raw --stylize 50 --no character, person, fist, logo
```

Other sticker words (one per sticker, ≤ 3 words): "COMBO ×{n}", "STAGE CLEAR", "拳!", "INSERT COIN". Avoid "FIGHT!", "PERFECT", "YOU WIN", "ROUND 1".

---

## 4. Per-post quick prompts

The calendar references templates by ID. When a post needs a quick one-off, fill this and pick the tool:

```text
[Template {ID}] {subject}. Palette: bg #0b0b10, accent #ff5a36, burst #ffd23f, cool #4cc9ff. Style: {crisp 16-bit pixel art | cel-shaded anime background | flat editorial}. Composition: focal point {right|center}, empty {left|top} area for text added later. Exclude: text, logos, people, arms, characters.
```

- Midjourney: turn "Exclude:" into `--no text, logo, person, arm, character` and append `--ar {w:h} --raw --stylize {50–250}`.
- Flux: delete the "Exclude:" line; add "empty scene … sharp focus throughout" and tie each hex to an object.
- GPT Image: keep it as written; set `size` from §2.

---

## 5. Generation log

Record every image that gets posted. Newest on top.

| Date | Template | Tool / model | Seed | Prompt (or link to it) | File | Used in |
| --- | --- | --- | --- | --- | --- | --- |
| | | | | | | |
