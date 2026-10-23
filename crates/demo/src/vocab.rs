//! Word banks for the fake Contoso org. All names are invented.

pub const ME: (&str, &str) = ("Ada Lindqvist", "ada.lindqvist@contoso.com");

pub const PEOPLE: &[&str] = &[
    "Priya Shah",
    "Marcus Oyelaran",
    "Hannah Becker",
    "Diego Alvarez",
    "Mei Tanaka",
    "Tom Gallagher",
    "Fatima Haddad",
    "Lukas Novak",
    "Chloé Martin",
    "Sanjay Iyer",
    "Olivia Brooks",
    "Kenji Watanabe",
    "Amara Nwosu",
    "Jonas Eriksen",
    "Isabela Costa",
    "Ravi Menon",
    "Sofia Rossi",
    "Ethan Clarke",
    "Yara Haddad",
    "Nikolai Petrov",
    "Grace Kim",
    "Mateo Fernández",
    "Leila Farahani",
    "Owen Murphy",
    "Aisha Bello",
    "Felix Wagner",
    "Nora Lindgren",
    "Samuel Adeyemi",
    "Zoë van Dijk",
    "Hiroshi Sato",
    "Camila Reyes",
    "Arjun Kapoor",
    "Elena Popescu",
    "Ben Carter",
    "Lina Johansson",
    "Kwame Mensah",
];

pub const GROUPS: &[&str] = &[
    "[Payments]\\Payments Team",
    "[Storefront]\\Web Platform",
    "[Platform]\\SRE",
];

/// (project, repos)
pub const PROJECTS: &[(&str, &[&str])] = &[
    (
        "Payments",
        &[
            "payments-api",
            "ledger-service",
            "fraud-rules",
            "payouts-worker",
        ],
    ),
    (
        "Storefront",
        &[
            "checkout-web",
            "storefront-web",
            "catalog-service",
            "search-indexer",
        ],
    ),
    (
        "Platform",
        &[
            "infra-terraform",
            "build-tools",
            "identity-gateway",
            "observability",
        ],
    ),
];

pub const PR_VERBS: &[&str] = &[
    "Fix",
    "Add",
    "Refactor",
    "Remove",
    "Harden",
    "Speed up",
    "Migrate",
    "Handle",
    "Bump",
    "Document",
    "Split",
    "Cache",
    "Retry",
    "Validate",
    "Instrument",
    "Simplify",
];

pub const PR_OBJECTS: &[&str] = &[
    "flaky checkout retry",
    "idempotency keys on charge calls",
    "currency rounding in refunds",
    "3DS challenge timeout",
    "ledger reconciliation job",
    "webhook signature validation",
    "payout batching window",
    "fraud score threshold config",
    "cart merge after sign-in",
    "address autocomplete debounce",
    "promo code stacking rules",
    "product image lazy loading",
    "search facet counts",
    "catalog sync backpressure",
    "stale price cache",
    "Terraform state locking",
    "build cache eviction",
    "OIDC token refresh",
    "rate limiter for public API",
    "structured logging fields",
    "trace sampling for checkout",
    "health check endpoints",
    "database connection pool sizing",
    "feature flag cleanup",
    "CI matrix for Node 22",
    "Rust toolchain to 1.95",
    "dead letter queue alerts",
    "session cookie SameSite policy",
    "PDF invoice rendering",
    "tax calculation for EU VAT",
    "inventory reservation expiry",
    "gift card balance check",
    "order history pagination",
    "accessibility labels on checkout form",
    "dark mode tokens in storefront",
    "Kafka consumer rebalancing",
    "SLO dashboard queries",
    "secret rotation runbook",
    "e2e smoke tests for payouts",
    "mobile viewport layout on PDP",
];

pub const PR_SUFFIXES: &[&str] = &[
    "",
    "",
    "",
    " (follow-up)",
    " behind feature flag",
    " for EU region",
    " in hot path",
    " — part 2",
];

pub const BRANCH_PREFIXES: &[&str] = &["feature", "fix", "chore", "users"];

pub const PR_DESCRIPTIONS: &[&str] = &[
    "## Why\nWe saw intermittent failures in production during peak traffic.\n\n## What\n- Tighten the retry budget\n- Add metrics for attempts\n\n## Testing\nUnit tests plus a soak run in staging for 2h.",
    "Small cleanup before the bigger migration lands. No behavior change intended.",
    "## Summary\nImplements the design from the RFC discussed on Tuesday.\n\nRollout is behind `checkout.v2` and defaults off.",
    "Fixes the regression introduced last sprint. Added a test that fails on main.",
    "Bumps the dependency and fixes the three call sites that broke.\n\nChangelog reviewed; nothing else affects us.",
    "## Context\nOn-call noticed the alert firing every night at 02:00 UTC.\n\n## Change\nMoves the job to a jittered schedule and adds a lock.",
];

pub const COMMENTS: &[&str] = &[
    "Nice, this reads much better than before.",
    "Could we add a test for the empty case?",
    "nit: rename this to something more descriptive?",
    "Is this safe under concurrent requests? I think two workers could race here.",
    "Why not reuse the existing helper in `common/`?",
    "LGTM once CI is green.",
    "This changes the public contract — do we need a version bump?",
    "Can you split the refactor from the behavior change? Hard to review together.",
    "Good catch. Fixed in the latest push.",
    "Done.",
    "I'd prefer we log at debug here, this path is hot.",
    "What happens if the upstream returns 429?",
    "Should this be behind the feature flag as well?",
    "Agreed, let's track it as a follow-up.",
    "I tested this locally against staging and it works.",
    "Can we avoid the extra allocation in the loop?",
    "The error message could say which field failed validation.",
    "Let's keep the old endpoint for one more release.",
];

pub const MENTION_COMMENTS: &[&str] = &[
    "{me} could you take a look at the retry logic before we merge?",
    "Looping in {me} since you wrote the original version of this.",
    "{me} does this match what we agreed in the design review?",
    "cc {me} — this touches the ledger schema.",
    "{me} can you confirm the rollout plan for EU?",
    "Pinging {me} for a second pair of eyes on the migration.",
];

pub const WI_BUG: &[&str] = &[
    "Checkout spinner never stops when card is declined",
    "Refund amount off by one cent for JPY",
    "Search returns no results for accented product names",
    "Order confirmation email sent twice",
    "Payout report CSV has wrong column order",
    "Login loops on Safari with third-party cookies blocked",
    "Cart badge count stale after removing item",
    "Webhook retries hammer merchant endpoint",
    "Timezone bug in daily settlement job",
    "Promo code accepted after expiry",
    "Product page crashes when image list is empty",
    "Tax line missing on invoices for Norway",
    "Memory leak in catalog sync worker",
    "Build agent runs out of disk on large monorepo checkout",
    "Terraform plan shows drift on every run",
];

pub const WI_STORY: &[&str] = &[
    "As a shopper I can save my card for next time",
    "As a merchant I can export payouts by date range",
    "As support I can see why a payment failed",
    "As a shopper I can pay with Apple Pay",
    "As a merchant I can configure partial refunds",
    "As a shopper I get address suggestions while typing",
    "As finance I can reconcile ledger entries daily",
    "As a developer I can run the checkout flow locally",
    "As on-call I get paged only for customer-facing errors",
    "As a shopper I can filter search results by availability",
];

pub const WI_TASK: &[&str] = &[
    "Write migration for new column",
    "Add metrics and dashboard",
    "Update API docs",
    "Add integration test",
    "Remove old feature flag",
    "Load test in staging",
    "Review threat model",
    "Wire up alerting",
    "Update runbook",
    "Spike: evaluate library options",
];

pub const WI_FEATURE: &[&str] = &[
    "Saved payment methods",
    "Partial refunds",
    "Real-time fraud scoring",
    "Faster search relevance",
    "Checkout v2",
    "Self-serve payout exports",
    "Zero-downtime deploys",
    "Unified observability",
];

pub const WI_EPIC: &[&str] = &[
    "EU expansion",
    "Platform reliability 2026",
    "Merchant self-service",
    "Mobile-first storefront",
];

pub const WI_COMMENTS: &[&str] = &[
    "Reproduced on staging with a test card.",
    "Moving to next sprint, blocked on the API change.",
    "Root cause is the retry policy, see the linked PR.",
    "Customer reported this again today.",
    "Deployed the fix to canary; watching metrics.",
];

pub const PIPELINES: &[(&str, &str)] = &[
    ("payments-api CI", "\\Payments"),
    ("payments-api Deploy", "\\Payments\\Release"),
    ("ledger-service CI", "\\Payments"),
    ("fraud-rules Nightly", "\\Payments"),
    ("checkout-web CI", "\\Storefront"),
    ("storefront-web E2E", "\\Storefront"),
    ("catalog-service CI", "\\Storefront"),
    ("search-indexer Reindex", "\\Storefront\\Jobs"),
    ("infra-terraform Plan", "\\Platform"),
    ("build-tools CI", "\\Platform"),
    ("identity-gateway CI", "\\Platform"),
    ("observability Deploy", "\\Platform\\Release"),
];

pub const SPRINTS: &[&str] = &["Sprint 41", "Sprint 42", "Sprint 43", "Sprint 44"];

pub const AREAS: &[&str] = &[
    "Checkout", "Ledger", "Payouts", "Search", "Catalog", "Infra", "Identity",
];

pub const TAGS: &[&str] = &[
    "customer-reported",
    "tech-debt",
    "security",
    "perf",
    "a11y",
    "eu",
    "on-call",
    "quick-win",
];
