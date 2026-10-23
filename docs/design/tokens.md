# Design tokens, themes, backgrounds

Status: v0.1 contract. Owner: ux-designer. Consumers: `crates/theme`, `crates/ui`, every view.
Related: [principles.md](principles.md), [sound.md](sound.md), [ken.md](ken.md), spec `docs/specs/0007-*` (theming, backgrounds, sounds: behavior + acceptance).

Rule from AGENTS.md: **no hard-coded colors, spacing, motion, or sound in components.** Every value below is reached through a token. A component that needs a value that doesn't exist adds a token here first.

## 1. Token model

| Layer | User-overridable in v0.1 | Where it lives |
|---|---|---|
| Color (incl. syntax, domain, ken) | Yes, per theme file | `[variant.colors]` in `*.adoken-theme.toml` |
| Accent | Yes, preset in settings or via theme colors | `settings.toml [appearance]` |
| Typography family + scale | Family + global scale only | `settings.toml [appearance]` |
| Spacing, radius, sizes, elevation, z-layers | No (built-in constants) | `crates/theme` constants |
| Motion | No (constants); reduce-motion switch only | `crates/theme` constants |
| Sound | On/off + master volume | [sound.md](sound.md) |
| Background | Yes | `settings.toml [background]` |

Naming rules:
- Lowercase dotted keys: `<group>.<role>[.<variant>]`, snake_case segments (`bg.selected_muted`).
- **No key is a prefix of another key** (TOML cannot hold both `a.b = "…"` and `a.b.c = "…"`). Use `_` to extend a leaf: `accent.solid` + `accent.solid_hover`.
- Values: `#rrggbb`, `#rrggbbaa`, or an alias `"@other.key"`. Aliases are resolved after all layers merge, so overriding `status.info.fg` also recolors everything aliased to it. Unknown alias target or cycle → validation error; that key falls back to the built-in variant of the same appearance.
- Alpha colors (`#rrggbbaa`) are composited over the surface they're painted on; they never stack more than once (hover on a selected row paints `bg.selected`, then `bg.hover`, nothing else).

## 2. Color tokens

Built-in family `adoken`: variants `adoken-dark` (display name **Dojo Night**) and `adoken-light` (**Dojo Day**). Values below are the built-in source of truth and were contrast-checked (WCAG 2.x relative luminance, alpha flattened over the named base).

### 2.1 Surfaces

| Token | Dojo Night | Dojo Day | Use |
|---|---|---|---|
| `bg.app` | `#0E0F13` | `#F6F6F8` | Window base. Replaced by the custom background when enabled. |
| `bg.sidebar` | `#111217` | `#EFEFF3` | Sidebar / rail. |
| `bg.surface` | `#15161C` | `#FFFFFF` | List + detail panes. |
| `bg.raised` | `#1B1D24` | `#FFFFFF` | Cards, sticky headers, table headers. |
| `bg.overlay` | `#1F2129` | `#FFFFFF` | Palette, menus, dialogs, toasts. |
| `bg.scrim` | `#05060899` | `#15161C52` | Backdrop behind modal overlays. |
| `bg.input` | `#101116` | `#FFFFFF` | Text fields. |
| `bg.code` | `#121318` | `#FBFBFC` | Diff + code blocks. **Always painted opaque**, even over backgrounds. |
| `bg.hover` | `#FFFFFF0F` | `#15161C0A` | Hover wash. |
| `bg.active` | `#FFFFFF17` | `#15161C14` | Pressed wash. |
| `bg.selected` | `#F26B3A29` | `#E0531F1F` | Selected row, list focused. Derived from accent (§3). |
| `bg.selected_muted` | `#FFFFFF12` | `#15161C0F` | Selected row, list not focused. |

### 2.2 Borders

| Token | Dojo Night | Dojo Day | Use |
|---|---|---|---|
| `border.subtle` | `#FFFFFF0F` | `#15161C0F` | Row separators. Decorative. |
| `border.default` | `#FFFFFF1A` | `#15161C1F` | Pane edges, cards, overlays. |
| `border.strong` | `#FFFFFF2E` | `#15161C33` | Dividers that must read at a glance. |
| `border.control` | `#6A6E7E` | `#858995` | Checkbox/radio/toggle-off outlines, input outline on hover. ≥3:1 vs every bg. |
| `border.focus` | `#FF8A5F` | `#C24416` | Keyboard focus ring. ≥3:1 vs every bg. Derived from accent. |

### 2.3 Text

| Token | Dojo Night | Dojo Day | Use | Min contrast |
|---|---|---|---|---|
| `text.primary` | `#ECEDF1` | `#15161C` | Titles, body. | 4.5 |
| `text.secondary` | `#A6A9B6` | `#51545F` | Metadata, labels. | 4.5 |
| `text.tertiary` | `#9296A6` | `#5F636E` | Timestamps, placeholders, hints. | 4.5 on every bg incl. selected/hover/washes |
| `text.disabled` | `#5A5E6C` | `#A3A6B0` | Disabled controls only. | exempt |
| `text.link` | `#8FB4FF` | `#1F5FD1` | Links. | 4.5 |
| `text.match` | `#FF8A5F` | `#B8410F` | Fuzzy-match characters in palette/search. Derived from accent. | 4.5 |

Placeholders use `text.tertiary` (not disabled) so they stay readable.

### 2.4 Accent

| Token | Dojo Night | Dojo Day | Use |
|---|---|---|---|
| `accent.solid` | `#F26B3A` | `#C24416` | Primary buttons, toggle-on, progress fill. |
| `accent.solid_hover` | `#FF7B4B` | `#AD3B12` | Hovered primary. |
| `accent.on` | `#1A0A04` | `#FFFFFF` | Text/icon on `accent.solid` (6.4:1 / 5.1:1). |
| `accent.fg` | `#FF8A5F` | `#B8410F` | Accent text/icons on surfaces. |
| `accent.wash` | `#F26B3A1F` | `#E0531F14` | Tinted chips, callouts. |

The default accent is **Shu** (朱, vermilion) — also the brand color for adoken.dev.

### 2.5 Status

| Token | Dojo Night | Dojo Day |
|---|---|---|
| `status.success.fg` | `#3FD18A` | `#0F7A4A` |
| `status.success.wash` | `#3FD18A1F` | `#12804F14` |
| `status.warning.fg` | `#F0B545` | `#94600A` |
| `status.warning.wash` | `#F0B5451F` | `#94600A14` |
| `status.danger.fg` | `#FF5C6C` | `#C8283A` |
| `status.danger.wash` | `#FF5C6C1F` | `#C8283A14` |
| `status.danger.solid` | `#D63A40` | `#C8283A` |
| `status.danger.on` | `#FFFFFF` | `#FFFFFF` |
| `status.info.fg` | `#62A8FF` | `#1A66C9` |
| `status.info.wash` | `#62A8FF1F` | `#1A66C914` |
| `shadow.color` | `#000000` | `#15161C` |

`*.fg` passes 4.5:1 on its own `*.wash` and on every surface. Status is **never color-only**: every status also has an icon and/or a text label (colorblind-safe; vermilion accent and danger red are intentionally distinguished by shape, not hue alone).

### 2.6 Diff

| Token | Dojo Night | Dojo Day | Note |
|---|---|---|---|
| `diff.add.bg` | `#3FD18A1F` | `#12804F1A` | Line wash over `bg.code`. |
| `diff.add.word` | `#3FD18A3D` | `#12804F26` | Intraline changed range. |
| `diff.add.gutter` | `@status.success.fg` | `@status.success.fg` | `+` sign, 2 px gutter bar. |
| `diff.del.bg` | `#FF5C6C1F` | `#C8283A1A` | |
| `diff.del.word` | `#FF5C6C3D` | `#C8283A26` | |
| `diff.del.gutter` | `@status.danger.fg` | `@status.danger.fg` | `−` sign, gutter bar. |
| `diff.hunk.bg` | `#62A8FF14` | `#1A66C90F` | Hunk header row. |
| `diff.hunk.fg` | `@text.link` | `@text.link` | |
| `diff.comment` | `@status.warning.fg` | `@status.warning.fg` | Thread marker in gutter. |

Only `text.primary` and `syntax.*` are drawn on `diff.*.word`. Every `syntax.*` value below passes 4.5:1 on `bg.code` and on each `diff.*` wash flattened over it.

### 2.7 Syntax

Keys map from tree-sitter capture names (first segment match: `@keyword.return` → `syntax.keyword`). Unmapped captures render `syntax.variable`.

| Token | Dojo Night | Dojo Day | Style |
|---|---|---|---|
| `syntax.keyword` | `#FF8A5F` | `#A3360B` | |
| `syntax.string` | `#9BDB8A` | `#276A19` | |
| `syntax.number` | `#FFC46B` | `#7A4B00` | |
| `syntax.constant` | `@syntax.number` | `@syntax.number` | |
| `syntax.comment` | `#A0A4B3` | `#565A64` | italic |
| `syntax.function` | `#8FB4FF` | `#1A56BF` | |
| `syntax.type` | `#6FD6D6` | `#095F5F` | |
| `syntax.variable` | `#ECEDF1` | `#15161C` | |
| `syntax.property` | `#C9B6FF` | `#6040C8` | |
| `syntax.operator` | `#C2C5D0` | `#3D404A` | |
| `syntax.punctuation` | `#A6A9B6` | `#51545F` | |
| `syntax.tag` | `#FF8A9A` | `#A82236` | |
| `syntax.attribute` | `@syntax.number` | `@syntax.number` | |

### 2.8 Domain (Azure DevOps state)

Built-ins define these as aliases so themes only need to override status colors. Every domain color is paired with an icon glyph (listed) — color is never the sole signal.

| Token | Default (both) | Icon (Lucide name) | ADO source |
|---|---|---|---|
| `pr.active` | `@status.info.fg` | `git-pull-request` | status `active` |
| `pr.draft` | `@text.tertiary` | `git-pull-request-draft` | `isDraft = true` |
| `pr.completed` | `#A78BFA` / `#6D4AD8` | `git-merge` | status `completed` |
| `pr.abandoned` | `@text.disabled` + label "Abandoned" | `git-pull-request-closed` | status `abandoned` |
| `vote.approved` | `@status.success.fg` | `badge-check` | vote `10` |
| `vote.approved_suggestions` | `@status.success.fg` | `circle-check-big` | vote `5` |
| `vote.waiting` | `@status.warning.fg` | `clock` | vote `-5` |
| `vote.rejected` | `@status.danger.fg` | `octagon-x` | vote `-10` |
| `vote.none` | `@text.tertiary` | `circle-dashed` | vote `0` |
| `pipeline.succeeded` | `@status.success.fg` | `circle-check` | result `succeeded` |
| `pipeline.failed` | `@status.danger.fg` | `circle-x` | result `failed` |
| `pipeline.partial` | `@status.warning.fg` | `circle-alert` | result `partiallySucceeded` |
| `pipeline.running` | `@status.info.fg` | `loader-circle` (static under reduce-motion) | status `inProgress` |
| `pipeline.queued` | `@text.tertiary` | `circle-dot-dashed` | status `notStarted` |
| `pipeline.canceled` | `@text.tertiary` | `circle-slash` | result `canceled` |
| `wi.bug` | `#FF6B6B` / `#C0303A` | `bug` | type Bug |
| `wi.task` | `@status.warning.fg` | `square-check` | Task |
| `wi.story` | `@status.info.fg` | `book-open` | User Story / Product Backlog Item / Requirement |
| `wi.feature` | `@pr.completed` | `trophy` | Feature |
| `wi.epic` | `#FF9F43` / `#A65200` | `crown` | Epic |
| `wi.issue` | `#E879F9` / `#A21CAF` | `circle-alert` | Issue / Impediment |
| `wi.other` | `@text.secondary` | `square` | any custom type |
| `priority.p1` | `@status.danger.fg` | `chevrons-up` | Priority 1 / notification priority urgent |
| `priority.p2` | `@status.warning.fg` | `chevron-up` | 2 / high |
| `priority.p3` | `@text.secondary` | `minus` | 3 / normal |
| `priority.p4` | `@text.tertiary` | `chevron-down` | 4 / low |

Icons: Lucide (ISC). Record in `THIRD_PARTY_NOTICES.md` when bundled.

### 2.9 Ken (arcade layer)

Used only by the opt-in layer in [ken.md](ken.md) and by the fist mark loader (`adoken_ui::loaders`: outline, base, wrap; highlight and shade are the base mixed toward `ken.spark` / `ken.outline`). Large display text only (≥ 24 px bold), so the minimum is 3:1.

| Token | Dojo Night | Dojo Day | Use |
|---|---|---|---|
| `ken.primary` | `#FF5A2E` | `#D9431A` | Combo numerals, K.O. letter fill. |
| `ken.secondary` | `#FFD23F` | `#B07A00` | Multiplier "×", streak flame core, bevel highlight. |
| `ken.outline` | `#1A0A04` | `#1A0A04` | 3 px stroke around arcade lettering (guarantees legibility on any background). |
| `ken.spark` | `#FFE8A3` | `#FFB547` | Particle sparks. |

### 2.10 Other built-in themes

Four built-ins ship in v0.0.1 (spec 0007 FR-2), each its own family file in `crates/theme/themes/`, loaded strict through the same parser as user themes and contrast-checked by the same rules (§9.3):

| Family | Variant id | Display name | Role |
|---|---|---|---|
| `adoken` | `adoken-dark` | Dojo Night | Signature dark (default dark). |
| `adoken` | `adoken-light` | Dojo Day | Light (default light). |
| `arcade` | `arcade-dark` | Arcade | High-contrast dark: neon on near-black, text ramp ≥ 7:1 (WCAG AAA) on every surface, hover and selection wash; yellow focus ring. |
| `sakura` | `sakura-light` | Sakura | Soft light: blossom paper, plum ink, cherry accent. |

Community built-ins (18 families, 29 variants: Catppuccin, Tokyo Night, Dracula, GitHub, Ayu, Gruvbox, Rosé Pine, Nord, One Dark Pro, Atom One Dark, Night Owl, Winter is Coming, Palenight, SynthWave '84, Cobalt2, Andromeda, Dark+/Light+, Zeron) follow the four above in picker order. They are generated by `crates/theme/examples/port_zeron_themes.rs` from zeron's curated seed palettes through `adoken_theme::draft`: surfaces from editor/sidebar/panel colors, washes and borders at the Dojo alphas, `bg.selected`/`accent.*` from the accent (§3), status and syntax from the theme, domain tokens aliased as in §2.8. Contrast enforcement (§9.3, OKLab lightness steps) then fixes what fails, and each file lists those adjustments in its header. Gallery snapshots cover every story in the four house themes; the `theme-strip` story paints a miniature of every built-in. Licenses: THIRD_PARTY_NOTICES.md.

Values for Arcade and Sakura (aliases as written in the files; roles and minimums as in §2.1–§2.9):

| Token | Arcade | Sakura |
|---|---|---|
| `bg.app` | `#07060B` | `#FBF6F7` |
| `bg.sidebar` | `#0A0910` | `#F5ECEF` |
| `bg.surface` | `#0D0C14` | `#FFFFFF` |
| `bg.raised` | `#14121D` | `#FFFFFF` |
| `bg.overlay` | `#181624` | `#FFFFFF` |
| `bg.scrim` | `#000000B3` | `#2A1A2052` |
| `bg.input` | `#09080E` | `#FFFFFF` |
| `bg.code` | `#0B0A11` | `#FDF9FA` |
| `bg.hover` | `#FFFFFF14` | `#3A1F2A0A` |
| `bg.active` | `#FFFFFF24` | `#3A1F2A14` |
| `bg.selected` | `#FF2E8833` | `#D6336C1A` |
| `bg.selected_muted` | `#FFFFFF1A` | `#3A1F2A0F` |
| `border.subtle` | `#FFFFFF1F` | `#3A1F2A12` |
| `border.default` | `#FFFFFF33` | `#3A1F2A21` |
| `border.strong` | `#FFFFFF59` | `#3A1F2A38` |
| `border.control` | `#8C88A8` | `#8A7780` |
| `border.focus` | `#FFE14D` | `#B8235A` |
| `text.primary` | `#FFFFFF` | `#2A1A20` |
| `text.secondary` | `#D9D6EA` | `#5E4A53` |
| `text.tertiary` | `#BDB9D3` | `#6B5760` |
| `text.disabled` | `#6E6A85` | `#B3A3AA` |
| `text.link` | `#6EE7FF` | `#2F5FB8` |
| `text.match` | `#FFE14D` | `#A81E52` |
| `accent.solid` | `#FF2E88` | `#B8235A` |
| `accent.solid_hover` | `#FF4F9D` | `#A11D4E` |
| `accent.on` | `#0A0006` | `#FFFFFF` |
| `accent.fg` | `#FF78B6` | `#A81E52` |
| `accent.wash` | `#FF2E8826` | `#D6336C14` |
| `status.success.fg` | `#4DFF9A` | `#1D7A4C` |
| `status.success.wash` | `#4DFF9A26` | `#1D7A4C14` |
| `status.warning.fg` | `#FFD23F` | `#8A5A08` |
| `status.warning.wash` | `#FFD23F26` | `#8A5A0814` |
| `status.danger.fg` | `#FF6B85` | `#C0283F` |
| `status.danger.wash` | `#FF6B8526` | `#C0283F14` |
| `status.danger.solid` | `#E0204A` | `#C0283F` |
| `status.danger.on` | `#FFFFFF` | `#FFFFFF` |
| `status.info.fg` | `#5CC8FF` | `#2562B8` |
| `status.info.wash` | `#5CC8FF26` | `#2562B814` |
| `shadow.color` | `#000000` | `#2A1A20` |
| `diff.add.bg` | `#4DFF9A1F` | `#1D7A4C1A` |
| `diff.add.word` | `#4DFF9A40` | `#1D7A4C26` |
| `diff.add.gutter` | `@status.success.fg` | `@status.success.fg` |
| `diff.del.bg` | `#FF6B851F` | `#C0283F1A` |
| `diff.del.word` | `#FF6B8540` | `#C0283F26` |
| `diff.del.gutter` | `@status.danger.fg` | `@status.danger.fg` |
| `diff.hunk.bg` | `#5CC8FF1A` | `#2562B80F` |
| `diff.hunk.fg` | `@text.link` | `@text.link` |
| `diff.comment` | `@status.warning.fg` | `@status.warning.fg` |
| `syntax.keyword` | `#FF8FC4` | `#A3245A` |
| `syntax.string` | `#B8FF6E` | `#3B6B1E` |
| `syntax.number` | `#FFD23F` | `#8A4B00` |
| `syntax.constant` | `@syntax.number` | `@syntax.number` |
| `syntax.comment` | `#B4B0CA` | `#6B5760` |
| `syntax.function` | `#6EE7FF` | `#2A58B0` |
| `syntax.type` | `#7CFFE0` | `#0E6468` |
| `syntax.variable` | `#FFFFFF` | `#2A1A20` |
| `syntax.property` | `#D6B3FF` | `#6A3FB8` |
| `syntax.operator` | `#E6E3F5` | `#4A3A42` |
| `syntax.punctuation` | `#C8C5DA` | `#5E4A53` |
| `syntax.tag` | `#FF9A7A` | `#B02A3A` |
| `syntax.attribute` | `@syntax.number` | `@syntax.number` |
| `pr.active` | `@status.info.fg` | `@status.info.fg` |
| `pr.draft` | `@text.tertiary` | `@text.tertiary` |
| `pr.completed` | `#C49BFF` | `#7045C4` |
| `pr.abandoned` | `@text.disabled` | `@text.disabled` |
| `vote.approved` | `@status.success.fg` | `@status.success.fg` |
| `vote.approved_suggestions` | `@status.success.fg` | `@status.success.fg` |
| `vote.waiting` | `@status.warning.fg` | `@status.warning.fg` |
| `vote.rejected` | `@status.danger.fg` | `@status.danger.fg` |
| `vote.none` | `@text.tertiary` | `@text.tertiary` |
| `pipeline.succeeded` | `@status.success.fg` | `@status.success.fg` |
| `pipeline.failed` | `@status.danger.fg` | `@status.danger.fg` |
| `pipeline.partial` | `@status.warning.fg` | `@status.warning.fg` |
| `pipeline.running` | `@status.info.fg` | `@status.info.fg` |
| `pipeline.queued` | `@text.tertiary` | `@text.tertiary` |
| `pipeline.canceled` | `@text.tertiary` | `@text.tertiary` |
| `wi.bug` | `#FF6B6B` | `#BE2F3F` |
| `wi.task` | `@status.warning.fg` | `@status.warning.fg` |
| `wi.story` | `@status.info.fg` | `@status.info.fg` |
| `wi.feature` | `@pr.completed` | `@pr.completed` |
| `wi.epic` | `#FFA24D` | `#A65200` |
| `wi.issue` | `#F08CFF` | `#A0268F` |
| `wi.other` | `@text.secondary` | `@text.secondary` |
| `priority.p1` | `@status.danger.fg` | `@status.danger.fg` |
| `priority.p2` | `@status.warning.fg` | `@status.warning.fg` |
| `priority.p3` | `@text.secondary` | `@text.secondary` |
| `priority.p4` | `@text.tertiary` | `@text.tertiary` |
| `ken.primary` | `#FF2E88` | `#D13A6E` |
| `ken.secondary` | `#FFE14D` | `#A87400` |
| `ken.outline` | `#0A0006` | `#2A0A16` |
| `ken.spark` | `#FFF3A8` | `#FF9EBB` |

## 3. Accent presets

`settings.toml`: `[appearance] accent = "theme" | "shu" | "ai" | "matcha" | "kin" | "sora" | "sakura"`. `"theme"` uses the theme file's `accent.*` values. A preset overrides these keys, derived at load:

| Preset | Base (dark) | Base (light) |
|---|---|---|
| `shu` 朱 vermilion (default) | `#F26B3A` | `#C24416` |
| `ai` 藍 indigo | `#7C83FF` | `#4B51D6` |
| `matcha` 抹茶 green | `#58C27D` | `#1E7A45` |
| `kin` 金 gold | `#E8B64C` | `#8A6100` |
| `sora` 空 sky | `#4FB3FF` | `#1669B8` |
| `sakura` 桜 pink | `#FF7AB6` | `#B8326F` |

Derivation (all in OKLCH, using the theme's `bg.surface`):
- `accent.solid` = base.
- `accent.solid_hover` = base with L +0.05 (dark) / −0.05 (light).
- `accent.on` = `#FFFFFF` or `#0E0F13`, whichever has higher contrast on `accent.solid`; if < 4.5, darken (light) / lighten (dark) `accent.solid` in L steps of 0.01 until ≥ 4.5.
- `accent.fg` = base with L raised (dark) / lowered (light) in 0.01 steps until ≥ 4.5 on `bg.overlay` and `bg.surface`.
- `accent.wash` = base at alpha `0x1F` (dark) / `0x14` (light).
- `bg.selected` = base at alpha `0x29` (dark) / `0x1F` (light).
- `text.match` = `border.focus` = `accent.fg`.

## 4. Typography

| Setting | Default | Options |
|---|---|---|
| `ui_font` | `".SystemUIFont"` (SF Pro on macOS, Segoe UI Variable on Windows) | any installed family; bundled fallback **Inter** (OFL 1.1) |
| `code_font` (was `mono_font`) | bundled **JetBrains Mono** v2.304 (OFL 1.1; static Regular, Italic, Bold) | `"system"` (SF Mono on macOS), any installed monospace family; a missing family falls back to JetBrains Mono (user decision 2026-09-30, spec 0009 §3 Code font) |
| `code_font_size` | `12` | `11`–`16` px; scales `type.code` and its line height (rounded to whole px), so diff rows follow |
| `code_ligatures` | `false` | `true` turns on `calt` in the code font |
| `font_scale` | `1.0` | `0.9`, `1.0`, `1.1`, `1.2`, `1.3` — multiplies every size, line height and row height below |

Scale (px at `font_scale = 1.0`; weights are GPUI `FontWeight` numeric values):

| Token | Size / line | Weight | Font | Use |
|---|---|---|---|---|
| `type.display` | 28 / 34 | 650 | ui | Onboarding headlines. |
| `type.title` | 20 / 26 | 600 | ui | View title, PR title in review. |
| `type.heading` | 15 / 20 | 600 | ui | Section heads, dialog titles. |
| `type.body` | 13 / 20 | 400 | ui | Default. |
| `type.body_strong` | 13 / 20 | 560 | ui | Row titles, emphasized body. |
| `type.small` | 12 / 16 | 400 | ui | Metadata, secondary rows. |
| `type.label` | 11 / 14 | 560 | ui | Group labels (sentence case, not all-caps), chips, badges. |
| `type.code` | 12 / 18 | 400 | code | Diff, code blocks. Size = `code_font_size`, line height × size / 12. |
| `type.code_inline` | 12 / 16 | 400 | code | Inline code, IDs (`!4821`, `#1234`). Code family, own size. |
| `type.kbd` | 11 / 14 | 500 | system mono | Shortcut keycaps (Menlo on macOS, whatever the code font). |
| `type.numeric` | inherits | inherits | ui, tabular figures | Counts, durations, timestamps in columns. |

Rules: minimum rendered size 11 px. Numbers in lists use tabular figures (`font_features: tnum`). Truncate with a trailing ellipsis; full text in tooltip and accessible label. Exception: path templates in the network log truncate with a **leading** ellipsis ("…/pullRequests/{n}/threads"), because the tail names the resource; full text in tooltip and row detail ([activity.md](activity.md) §5.2).

Ken lettering (K.O., combo numerals) is **not a font**: original vector SVG glyphs in `assets/ken/glyphs/` (see [ken.md](ken.md) §9), scaled via SVG transformations.

## 5. Spacing, sizes, radius

### 5.1 Spacing (4 px grid, names are px values)

`space.0 = 0`, `space.2 = 2`, `space.4 = 4`, `space.6 = 6`, `space.8 = 8`, `space.12 = 12`, `space.16 = 16`, `space.20 = 20`, `space.24 = 24`, `space.32 = 32`, `space.40 = 40`, `space.48 = 48`, `space.64 = 64`.

Defaults: icon↔label gap `space.6`; row horizontal padding `space.12`; pane padding `space.16`; section gap `space.24`; dialog padding `space.24`; onboarding card padding `space.40`.

### 5.2 Layout sizes

| Token | Value | Notes |
|---|---|---|
| `size.titlebar` | 38 | macOS traffic lights inset 12 px left. |
| `size.sidebar` | 232 (min 200, max 320) | Resizable, persisted. |
| `size.rail` | 56 | Collapsed sidebar (`Cmd+B`). |
| `size.list_pane` | 380 (min 300, max 560) | Resizable, persisted per view. |
| `size.statusbar` | 24 | |
| `size.row.compact` / `default` / `comfortable` | 32 / 40 / 52 | One-line rows by density. `size.row.compact` is in code as `size::ROW_COMPACT` (32): the one-line rows of anchored menus and pickers (state menu, people / tree pickers; [work-item-detail.md](work-item-detail.md) §3.3, §4.5). |
| `size.row2.compact` / `default` / `comfortable` | 48 / 56 / 68 | Two-line rows (title + meta). |
| `size.control.sm` / `md` / `lg` | 24 / 28 / 36 | Buttons, inputs, segmented controls. |
| `size.icon.xs` / `sm` / `md` / `lg` | 12 / 14 / 16 / 20 | Stroke 1.5 px at every size. |
| `size.avatar.sm` / `md` / `lg` | 16 / 20 / 28 | |
| `size.hit_min` | 24 × 24 | Minimum pointer target (WCAG 2.5.8). |
| `size.palette_width` | 680 (wide mode 960 when window ≥ 1200) | See [command-palette.md](command-palette.md). |
| `size.palette_row.compact` / `default` / `comfortable` | 36 / 44 / 52 | Palette result rows. |
| `size.badge` | 14 | Sidebar / rail count pill height. |
| `size.toast` | 360 | Toast stack width. |
| `size.action_menu_width` | 280 | Row actions menu width. |
| `size.pin_state_word_max` | 64 | Work-item state word cap in a pin row. |
| `size.pin_skeleton_bar` | 8 | Fetching pin title skeleton height. |
| `size.row.dense` | 24 (× `font_scale`) | `size::ROW_DENSE`. Network log rows, table header, privacy footer; independent of density ([activity.md](activity.md) §11). Activity drawer. |
| `size.drawer_min` | 160 | `size::DRAWER_MIN`. Activity drawer minimum height (spec 0016 FR-1). |
| `size.drawer_header` | 32 | `size::DRAWER_HEADER`. Activity drawer header and toolbars; fixed across densities. |
| `size.detail_wide_min` | 1000 | `size::DETAIL_WIDE_MIN`. Detail pane width at which the work item properties rail appears ([work-item-detail.md](work-item-detail.md) §2). Work item detail. |
| `size.detail_properties` | 280 | `size::DETAIL_PROPERTIES`. Work item properties rail width. |
| `size.detail_prose_max` | 720 | `size::DETAIL_PROSE_MAX`. Max width of work item title, description and comments. |
| `size.property_cell` | 52 | `size::PROPERTY_CELL`. Work item property grid cell height. |
| `size.picker_popover` | 320 | `size::PICKER_POPOVER`. Minimum width of anchored pickers (people, area, iteration) in `crates/ui`. |
| `size.sheet_width` | 480 | Form sheets (Move to project). v0.2 (slice C, spec 0015): not added in v0.1. |
| `size.coach_mark` | 320 | Guided tour coach mark width ([tours.md](tours.md) §9, spec 0020). |

`settings.toml [appearance] density = "compact" | "default" | "comfortable"`.

### 5.3 Radius

| Token | px | Use |
|---|---|---|
| `radius.xs` | 3 | Keycaps, inline code, tiny chips. |
| `radius.sm` | 6 | Buttons, inputs, selected rows (inset 4 px from pane edge). |
| `radius.md` | 8 | Cards, menus, toasts. |
| `radius.lg` | 12 | Palette, dialogs. |
| `radius.xl` | 16 | Onboarding card. |
| `radius.full` | 9999 | Pills, avatars, toggles. |

### 5.4 Opacity

| Token | Value | Use |
|---|---|---|
| `opacity.dimmed` | 0.5 | Whole elements that stay informative but step back, e.g. required reviewers who haven't voted in the PR list (user decision 2026-09-30). Never used to make text meet or miss contrast. |

## 6. Elevation

GPUI `BoxShadow { color, offset, blur_radius, spread_radius }`. Color = `shadow.color` with the listed alpha. Dark themes lean on borders; light themes lean on shadows.

| Token | Border | Shadows (dark alpha / light alpha) | Use |
|---|---|---|---|
| `elevation.0` | none | none | Panes. |
| `elevation.1` | `border.default` | `0 1 2 0` @ 0.24 / 0.06 | Cards, raised headers. |
| `elevation.2` | `border.default` | `0 2 6 0` @ 0.30 / 0.06 + `0 8 24 -4` @ 0.45 / 0.12 | Menus, tooltips, toasts. |
| `elevation.3` | `border.default` + top inner highlight 1 px `#FFFFFF0A` (dark only) | `0 4 12 0` @ 0.30 / 0.08 + `0 24 64 -8` @ 0.55 / 0.20 | Palette, dialogs. |

Z-layers (paint order, highest last): `z.base 0` · `z.sticky 10` · `z.dropdown 20` · `z.toast 30` · `z.palette 40` · `z.dialog 50` · `z.ken 60` (never takes focus, never intercepts pointer) · `z.onboarding 70` (full window).

## 7. Motion

### 7.1 Durations

| Token | ms | Use |
|---|---|---|
| `motion.instant` | 0 | Selection moves, focus ring, text input, tab switches. Keyboard speed beats smoothness. |
| `motion.fast` | 90 | Hover/press washes in; tooltips out. |
| `motion.quick` | 140 | Menus, tooltips in, palette in. |
| `motion.base` | 200 | Pane resize, expand/collapse, toasts in, list insertions. |
| `motion.slow` | 320 | View transitions, onboarding step change. |
| `motion.expressive` | 480 | Onboarding hero, Ken moments. |

Exits run ≈ ⅔ of the entrance, rounded to 10 ms, as explicit constants: `motion.quick_exit 90`, `motion.base_exit 130`, `motion.slow_exit 210`, `motion.expressive_exit 320`. Hover-out is 0 ms wash removal only when the pointer moves to a sibling row (avoid trailing ghosts), otherwise `motion.fast`.

Tooltips: show after 500 ms hover (0 ms if another tooltip was visible in the last 300 ms). `motion.hover_intent` = 150 ms: dwell before a hover-opened surface opens (pins peek, [pins.md](pins.md) §5.3; Z-7 submenus), same in every theme.

### 7.2 Easing

| Token | Curve | Use |
|---|---|---|
| `ease.out` | `cubic-bezier(0.16, 1, 0.3, 1)` | Entrances. |
| `ease.in` | `cubic-bezier(0.4, 0, 1, 1)` | Exits. |
| `ease.in_out` | `cubic-bezier(0.65, 0, 0.35, 1)` | Moves, resizes, crossfades. |
| `ease.linear` | linear | Progress bars, spinner rotation. |
| `spring.snappy` | k = 520, c = 36, m = 1 (ζ ≈ 0.79) | Checkmarks, chip insertion, toggle knob. |
| `spring.bouncy` | k = 300, c = 14, m = 1 (ζ ≈ 0.40) | Ken: combo pop, K.O. letter slam. |

Springs are implemented as a `Fn(f32) -> f32` easing over a precomputed duration (GPUI has no spring primitive): with ω = √(k/m), ζ = c / (2√(km)), ω_d = ω√(1−ζ²), `x(t) = 1 − e^(−ζωt)(cos ω_d t + (ζω/ω_d) sin ω_d t)`. Duration = first t after which |1 − x| < 0.001 permanently (≈ 368 ms snappy, ≈ 889 ms bouncy; within 2 % of rest by ≈ 160 ms and ≈ 490 ms); easing input `p ∈ [0,1]` maps to `t = p × duration`. Unit-test both against these durations.

### 7.3 Motion catalog

| Name | Properties | Duration / easing |
|---|---|---|
| `fade-in` | opacity 0→1, translate Y 4→0 | base, ease.out |
| `menu-in` | opacity 0→1, translate Y −4→0 | quick, ease.out |
| `menu-out` | opacity 1→0 | 90, ease.in |
| `palette-in` | opacity 0→1, translate Y −8→0; scrim 0→1 | quick, ease.out |
| `palette-out` | opacity 1→0; scrim 1→0 | 90, ease.in |
| `dialog-in` | opacity 0→1, translate Y 8→0 | base, ease.out |
| `toast-in` | opacity 0→1, translate Y 12→0 (bottom-right stack) | base, spring.snappy |
| `toast-out` | opacity 1→0, translate X 0→16 | 130, ease.in |
| `row-insert` | height 0→row, opacity 0→1 (only for rows inserted while visible; batch inserts > 5 rows appear without animation) | base, ease.out |
| `row-remove` | opacity 1→0 then height → 0 | 90 + 130, ease.in |
| `view-switch` | outgoing opacity 1→0 (90), incoming `fade-in` | slow total |
| `pane-resize` | width | base, ease.in_out (drag = instant) |
| `skeleton-shimmer` | wash sweep L→R | 1400 loop, ease.in_out, shared 30 fps clock |
| `peek-in` | opacity 0→1, translate X −4→0 (pins peek, [pins.md](pins.md) §5.2) | quick, ease.out |
| `peek-out` | opacity 1→0 | 90, ease.in |
| `glyph-spin` | rotation 0→360° on the `loader-circle` running glyph | 1000 loop, ease.linear, shared 30 fps clock; still under reduce motion |

Implementation notes:
- The pinned GPUI has no `div` scale transform; animate opacity + translate (relative `top`/`left` insets so siblings don't move). Scale is allowed only on SVG elements (Ken lettering, the celebration seal, [ken.md](ken.md) §12).
- Repeating animations (spinners, shimmer, Ken idle glow) run on one shared ~30 fps clock and only while mounted and visible; nothing repeats at display refresh rate.
- Input during an animation jumps it to its end state; animations never delay focus or actions.
- Never animate layout of a list the user is navigating by keyboard (no reflow under the cursor).

### 7.4 Reduce motion

`settings.toml [accessibility] reduce_motion = "system" | "on" | "off"` (default `system` = macOS "Reduce motion" / Windows "Show animations in Windows" off). Maps to GPUI `cx.set_reduce_motion()`; `with_animation` then snaps to end state automatically. Additionally, under reduce motion:
- All translate/scale/height animations snap; opacity changes snap too (no exceptions needed; GPUI snaps uniformly).
- Spinners render a static glyph + text ("Syncing…"). Determinate progress bars still update their value, without interpolation.
- Skeleton shimmer off (flat `bg.raised` blocks).
- Video backgrounds show the poster frame; Ken screen-shake and particles off, Ken overlays degrade to a toast (see [ken.md](ken.md)).
- Celebrations (spec 0022 FR-9) show the static Lucide `sparkles` glyph (14 px, `status.success.fg`) at their anchor for 3 s instead of confetti, sparks or the seal; a Milestone adds its toast.
- Onboarding uses its reduced variant (see [onboarding.md](onboarding.md) §7).

## 8. Sound tokens

Volumes are linear gain 0.0–1.0; final gain = `master × category × event × file`, where only `master` is a setting (spec 0025 FR-13). Details and event map: [sound.md](sound.md).

| Token | Default |
|---|---|
| `sound.enabled` | `false` (opt-in at onboarding) |
| `sound.master` | 0.6 |
| `sound.category.ui` | 0.35 |
| `sound.category.notify` | 0.7 |
| `sound.category.ken` | 0.6 |
| `sound.category.onboarding` | 0.5 |

## 9. Theme files

### 9.1 Location and loading

Config dir (`<config>`): macOS `~/Library/Application Support/adoken/`, Windows `%APPDATA%\adoken\`, Linux `$XDG_CONFIG_HOME/adoken/`.

- User themes: `<config>/themes/<family-id>.adoken-theme.toml`. Family id = file stem (`[a-z0-9-]{1,48}`).
- Built-ins ship as the same format, embedded in the binary, through the same loader.
- Hot reload: file watcher, 200 ms debounce. A reload that fails to parse keeps the last good version and surfaces an issue (below). Theme swap repaints in the next frame, no restart.
- Limits: 256 KB per file, 64 themes loaded.

### 9.2 Format

```toml
# <config>/themes/sumi.adoken-theme.toml
name = "Sumi"                  # family display name
author = "Your Name"           # optional, shown in picker
license = "CC0-1.0"            # optional SPDX id

[[variant]]
appearance = "dark"            # "dark" | "light" — required
name = "Sumi Ink"              # optional, default "<family name> Dark"
id = "sumi-ink"                # optional, default "<family-id>-<appearance>"

[variant.colors]
bg.app = "#0B0B0C"
bg.surface = "#121213"
text.primary = "#EDEBE6"
accent.solid = "#C8B37A"
accent.on = "#121213"
pr.completed = "@accent.fg"    # alias
# any key omitted falls back to the built-in adoken-dark value

[[variant]]
appearance = "light"
name = "Sumi Paper"

[variant.colors]
bg.app = "#F4F1EA"
bg.surface = "#FBF9F4"
text.primary = "#1B1A17"
```

TOML dotted keys (`bg.app`) and nested tables (`[variant.colors.bg] app = …`) are equivalent; the loader flattens both.

Resolution per variant: built-in variant of same appearance → theme file `colors` → accent preset (if `accent != "theme"`) → alias resolution → contrast enforcement → runtime `Theme`.

### 9.3 Validation

| Check | Severity | Behavior |
|---|---|---|
| TOML parse error | error | Theme not loaded (or last good version kept). |
| Missing `appearance`, duplicate variant ids | error | Variant skipped. |
| Bad hex, unknown alias target, alias cycle | error | That key falls back to built-in. |
| Unknown key | warning | Ignored (typo guard: suggest nearest key by edit distance ≤ 2, e.g. "Did you mean `text.primary`?"). |
| Contrast: `text.primary/secondary/tertiary/link/match`, `accent.fg`, `status.*.fg`, `syntax.*` < 4.5 on their bases (§2); `border.focus`, `border.control`, domain icons < 3.0; `accent.on`/`status.danger.on` < 4.5 on their solids | warning | If `[accessibility] enforce_contrast = true` (default), the foreground is adjusted in OKLCH L (0.01 steps, away from background) until it passes; the adjusted value is shown next to the original in the issue list. |

Issues surface in Settings › Appearance › "Theme issues (n)" and once per change as a toast: "Sumi has 2 issues" [Show]. Each issue: key, file:line, message, effective value.

Every built-in theme and every component story renders against the visual QA fixtures (sidebar, PR list, diff, palette, notification center, onboarding card, dialog, empty state) for snapshots.

### 9.4 Appearance settings

```toml
[appearance]
theme = "adoken-dark"          # one variant id, any appearance
accent = "theme"               # or a preset id, §3
density = "default"
font_scale = 1.0
ui_font = ".SystemUIFont"
code_font = "JetBrains Mono"   # "system" or an installed family (spec 0009 §3 Code font)
code_font_size = 12            # 11–16
code_ligatures = false
```

One theme, chosen in Settings › Appearance (user decision 2026-09-30): no light/dark pair and no following macOS. Until a theme is chosen, adoken starts in Dojo Night or Dojo Day to match the macOS appearance at launch. Files from before the change (`mode = "system" | "light" | "dark"`, `light_theme`, `dark_theme`) still load: the pair resolves to the theme that was showing (for `"system"`, the slot matching the current macOS appearance), and the next theme pick writes `theme` and drops the old keys. Other `[appearance]` keys are kept.

Picker (Settings › Appearance, ⌘P › "Theme: Choose…"): a search field and a grid of every theme as a card with a live miniature. Hovering or arrowing only moves the highlight; click or `Enter` applies and saves. No preview, no Dark/Light filter (search "dark" or "light" instead), no "Match system".

## 10. Custom backgrounds

v0.1: static images. v0.2 (per spec 0007): video. Both specified here so the model doesn't change.

### 10.1 Config

```toml
[background]
enabled = true
source = "~/Pictures/wallpapers/night-city.jpg"  # user path, or "builtin:dusk" | "builtin:lantern" | "builtin:ink"
fit = "cover"          # cover | contain | fill | tile | center
anchor = "center"      # center | top | bottom | left | right | top-left | top-right | bottom-left | bottom-right
blur = 24              # px at 1x, 0–64
dim = 0.35             # 0.0–0.9; black scrim on dark appearance, white on light
saturation = 1.0       # 0.0–1.5
tint = ""              # optional color or alias, e.g. "@accent.solid"
tint_strength = 0.0    # 0.0–0.5
surface_opacity = 0.82 # alpha of bg.sidebar / bg.surface / bg.raised over the image, 0.6–1.0
overlay_opacity = 0.96 # alpha of bg.overlay (palette, menus, dialogs), 0.9–1.0
legibility = "auto"    # auto | off (§10.4)

# video only (v0.2)
playback = "loop"      # loop | once-then-poster
fps_cap = 30           # 15 | 24 | 30
pause_when = ["unfocused", "occluded", "battery", "low_power", "reduce_motion"]

[background.dark]       # optional: per-appearance overrides (any key above)
source = "~/Pictures/wallpapers/night-city.jpg"

[background.light]
source = "~/Pictures/wallpapers/morning-shrine.jpg"
dim = 0.25

[background.views.pr_review]   # optional: per-view overrides (any key above)
blur = 48
surface_opacity = 0.94

[background.views.onboarding]
enabled = false
```

Precedence (last wins): `[background]` → `[background.<appearance>]` → `[background.views.<view>]`.

View ids: `onboarding`, `my_work`, `inbox`, `pull_requests`, `pr_review`, `work_items`, `pipelines`, `search_results`, `settings`. The palette/dialogs/menus are overlays, not views: they use the active view's background and `overlay_opacity`.

### 10.2 Paint stack (back to front)

1. Processed image (§10.3), fitted to the window content rect (under the titlebar too; the titlebar paints `bg.sidebar` at `surface_opacity`).
2. Tint quad: `tint` at `tint_strength`.
3. Dim quad: `#000000` (dark) / `#FFFFFF` (light) at effective `dim`.
4. Panes: `bg.sidebar`, `bg.surface`, `bg.raised` at `surface_opacity` (multiplied with the token's own alpha).
5. **`bg.code` always opaque.** Diffs and code never show the background through.
6. Overlays: `bg.overlay` at `overlay_opacity`, `elevation.3`.

The window-wide blur is baked into the texture (no per-frame blur, spec 0007 FR-4). Overlays additionally frost the area under them with the zui fork's backdrop blur (`OVERLAY_BLUR` = 16 px, only while a custom background shows, only inside the overlay's bounds; `crates/app/src/background/frost.rs`); they still stay ≥ 0.9 opaque. `WindowBackgroundAppearance` stays `Opaque` when a custom background is on.

Dim, tint, and opacity are paint-time quads: slider changes are live at frame rate. Blur, saturation, and fit are baked (§10.3).

### 10.3 Image pipeline

- Import: picking a file (drag onto Settings › Appearance › Background, or file picker) copies it to `<data>/backgrounds/<sha256>.<ext>` (data dir per `docs/adr/0001-local-store.md`); `source` stores the original path for display, the copy is what renders (moving/deleting the original doesn't break it). `<data>/backgrounds/imports.toml` maps each original path to its copy.
- Formats: PNG, JPEG, WebP, GIF (first frame). Max 40 MB, max 12000 × 12000. Larger → error "Image too large (max 40 MB)".
- Processing on a background executor, never on the UI thread:
  1. Decode; fit (`fit`/`anchor`) into the target = window content size × scale factor, same aspect as the window, capped at 3840 × 2400. Generated built-ins render directly at the target size.
  2. If `blur > 0`: downscale 4×, Gaussian blur with σ = `blur / 4`, upscale bilinear (visually identical for blur ≥ 8, ~16× cheaper). If `blur < 8`, blur at full bucket size.
  3. Apply saturation. Upload as one GPU image.
- Cache: the shown texture plus the previous one in memory (the previous one is evicted from the GPU while hidden, so a 5K window stays within budget) + disk cache `<data>/backgrounds/cache/<hash>.png` for user images (hash over source, fit, anchor, blur, saturation, size, scale; newest 4 kept). Generated built-ins are cheaper to regenerate than to decode, so they aren't disk-cached. Nothing runs before the first frame: paint `bg.app`, then crossfade the image in at `motion.base` once processed (reduce motion: snap).
- Window resize: keep painting the existing texture stretched; reprocess once the window settles, debounced 150 ms.
- GPU memory budget for backgrounds: ≤ 64 MB.

### 10.4 Legibility guarantee (`legibility = "auto"`)

After processing, compute luminance on a 64 × 64 downsample and take the 95th-percentile pixel `P95` (dark appearance) or 5th-percentile `P05` (light); transparent letterbox pixels (they show `bg.app`) are excluded. Simulate the paint stack over that pixel (tint, dim, then each of `bg.sidebar` / `bg.surface` / `bg.raised` at `surface_opacity`), then check `text.primary`, `text.secondary` and `text.tertiary` ≥ 4.5 and `border.focus` ≥ 3.0 against the result. If failing: raise `dim` in 0.05 steps up to 0.9; if still failing, raise `surface_opacity` in 0.02 steps up to 1.0. Settings shows "Adjusted for readability: dim 0.35 → 0.50" under the sliders. `legibility = "off"` skips this and shows a warning chip "Text may be hard to read" when the check fails.

### 10.5 Built-in backgrounds (original, procedural)

Generated at runtime from code (no bitmap assets, so no licensing surface) by `crates/app/src/background/procedural.rs`; the colors live in `adoken_theme::background::palette`:
- `builtin:dusk` — vertical gradient `#1B0F1E → #3A1420 → #F26B3A` (bottom 8 %), soft film grain (monochrome noise, 3 % opacity).
- `builtin:lantern` — two radial glows (`#F26B3A` at 30 %, 70 %; `#FFD23F` at 75 %, 25 %) over `#120C0A`, grain 3 %.
- `builtin:ink` — domain-warped value noise in `#0E0F13`…`#2A2C35` (sumi-e wash), grain 2 %.
All three look intentional at `blur = 0`; defaults `blur = 0`, `dim = 0.2`.

Demo mode (`ADOKEN_DEMO=1`, no `[background]` table in the demo `settings.toml`) shows an original procedural illustration instead — a dusk sky over layered ridges, a shrine gate and a blossoming branch — written once to `<demo dir>/backgrounds/demo-illustration-v1.png` and loaded through the user-image path (import, blur 6, dim 0.3, `surface_opacity` 0.7). It is not a `builtin:` source.

### 10.6 Video (v0.2)

- Formats: MP4/MOV (H.264, HEVC) decoded by the platform (AVFoundation on macOS, Media Foundation on Windows); Linux best-effort via GStreamer if present, otherwise the settings row explains "Video backgrounds need GStreamer on Linux". Max 200 MB, audio track ignored (always muted).
- Decode at ¼ resolution when `blur ≥ 8` (the downscale is the blur), otherwise at the bucket size; frames uploaded to one reusable GPU texture at ≤ `fps_cap`.
- Pause (show the current frame, stop decoding) whenever any `pause_when` condition holds: window unfocused, window occluded/minimized, on battery, OS low-power mode, reduce motion. Resume within one frame of the condition clearing.
- Poster frame = first frame, extracted at import and processed like an image; used for cold start, reduce motion, and paused states.
- Budget: video background must keep interaction frames < 16 ms and add < 5 % CPU on an M1 at 30 fps; otherwise it auto-drops to `fps_cap = 15` and shows "Lowered video frame rate to keep adoken fast".

### 10.7 States

| State | UI |
|---|---|
| No background | Settings row shows three built-in swatches + "Choose image…" + drop zone "Drop an image here". |
| Processing | Live preview thumbnail shows a skeleton; the window keeps the previous background until the new one is ready, then crossfades (`motion.base`). |
| Source missing (cache also gone) | Fall back to `bg.app`; settings row: "Background image not found" [Choose another] [Remove]. |
| Unsupported / too large | Inline error under the drop zone, previous background unchanged. |
| Legibility adjusted | Info line under sliders (§10.4). |

Privacy: backgrounds never leave the machine. Share/export features (e.g. Ken "fight card", [ken.md](ken.md)) exclude the custom background unless the user ticks "Include background".
