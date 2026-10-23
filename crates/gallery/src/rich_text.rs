//! `RichText` (spec 0015 FR-3, design §5.1): every construct, from
//! Markdown, and a live Azure DevOps HTML description beside it.

use std::sync::Arc;

use adoken_richtext::{Format, parse};
use adoken_theme::{radius, size, space, typography};
use adoken_ui::{ActiveTheme, MentionName, RichText, TypeStyleExt};
use gpui::{AnyElement, App, Window, div, prelude::*, px};

const ME: &str = "d2c4c61b-5a1a-6cf0-8a11-f7dd5385b607";
const SAM: &str = "6b3f0c2e-1d4a-4f5b-9c8d-7e6f5a4b3c2d";

const MARKDOWN: &str = "# Settlement totals use the merchant's day

Settlement runs at **00:05 UTC**, but merchants in *UTC+10* see the previous day's totals. \
Underline: <u>local midnight</u>; old cutoff ~~00:00~~; field `SettlementDay`. \
See [the runbook](https://contoso.example/runbook), #4821 and \
https://dev.azure.com/contoso/Storefront/_git/web/pullrequest/4295.

## Acceptance criteria

1. Totals use the merchant's calendar day
   - DST keeps the day **24 h** long
   - Backfill is *not* in scope
2. Reports show the time zone

- [x] Repro on staging
- [ ] Fix verified by @<6b3f0c2e-1d4a-4f5b-9c8d-7e6f5a4b3c2d>

> We settle in UTC because the bank does.

---

@<d2c4c61b-5a1a-6cf0-8a11-f7dd5385b607> can you verify? @<00000000-0000-0000-0000-000000000001> was on call.

```
select merchant_id, sum(amount) from settlements where day = :local_day group by merchant_id order by 2 desc;
```

| Region | Offset | Cutoff (local) | Settlement window | Owner | Notes |
| --- | --- | --- | --- | --- | --- |
| AU | +10 | 00:05 | 23:55 → 00:15 | Payments | Daylight saving from October |
| NZ | +12 | 00:05 | 23:55 → 00:15 | Payments | Chatham Islands are +12:45 |

![checkout screenshot](https://dev.azure.com/contoso/_apis/wit/attachments/9f1c?fileName=shot.png)
";

/// Work item 14's live description (`crates/sync/src/wire.rs` tests).
const LIVE_HTML: &str = "<div><div style=\"box-sizing:border-box;\"><span style=\"box-sizing:border-box;display:inline !important;\">The total order amount shown during checkout does not match the actual sum of the item prices in the cart.</span><br style=\"box-sizing:border-box;\"> </div><div style=\"box-sizing:border-box;\"><span style=\"box-sizing:border-box;display:inline !important;\">&#128310;</span> </div><div style=\"box-sizing:border-box;\"> </div><div style=\"box-sizing:border-box;\">workItemId<br> </div><br> </div><div> </div>";

fn names(id: &str) -> Option<MentionName> {
    match id {
        ME => Some(MentionName {
            name: "Dana Kim".into(),
            unique_name: Some("dana@contoso.com".into()),
            is_me: true,
        }),
        SAM => Some(MentionName {
            name: "Sam Okafor".into(),
            unique_name: Some("sam@contoso.com".into()),
            is_me: false,
        }),
        _ => None,
    }
}

pub fn story(window: &mut Window, cx: &mut App) -> AnyElement {
    // Kept across frames, as the detail keeps its per-(id, rev) cache.
    let docs = window.use_keyed_state("rich-text-docs", cx, |_, _| {
        (
            Arc::new(parse(MARKDOWN, Format::Markdown)),
            Arc::new(parse(LIVE_HTML, Format::Html)),
        )
    });
    let (markdown, html) = docs.read(cx).clone();
    let theme = cx.theme();
    let label = |text: &'static str| {
        div()
            .type_style(typography::LABEL, theme)
            .text_color(theme.colors.text_tertiary)
            .child(text)
    };
    let surface = || {
        div()
            .p(px(space::S16))
            .rounded(px(radius::MD))
            .border_1()
            .border_color(theme.colors.border_subtle)
            .bg(theme.colors.bg_surface)
    };
    div()
        .flex()
        .flex_row()
        .items_start()
        .gap(px(space::S16))
        .child(
            div()
                .w(px(560.0))
                .flex_none()
                .flex()
                .flex_col()
                .gap(px(space::S8))
                .child(label("MARKDOWN"))
                .child(
                    surface().child(RichText::new("rich-text-markdown", markdown).mentions(names)),
                ),
        )
        .child(
            div()
                .w(px(size::TOAST))
                .flex_none()
                .flex()
                .flex_col()
                .gap(px(space::S8))
                .child(label("AZURE DEVOPS HTML"))
                .child(surface().child(RichText::new("rich-text-html", html)))
                .child(label("CLIPPED AT 120"))
                .child(
                    surface().child(
                        RichText::new("rich-text-clipped", docs.read(cx).0.clone())
                            .mentions(names)
                            .max_height(Some(120.0)),
                    ),
                ),
        )
        .into_any_element()
}
