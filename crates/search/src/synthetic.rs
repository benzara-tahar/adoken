//! Deterministic ADO-shaped corpus and query set for tests, benches and the
//! perf harness (spec 0003 §6: 60% work items, 20% PRs, 5% pipelines,
//! 10% people, 5% repos/projects/commands).

use crate::{DocKind, SearchDoc};

/// SplitMix64: tiny, fast, and stable across platforms and releases.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len())]
    }

    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }
}

const COMPONENTS: &[&str] = &[
    "login",
    "SSO redirect",
    "token refresh",
    "pull request list",
    "work item form",
    "pipeline cache",
    "build agent",
    "release gate",
    "notification center",
    "search index",
    "dashboard",
    "payment gateway",
    "invoice export",
    "user profile",
    "audit log",
    "feature flags",
    "rate limiter",
    "webhook handler",
    "API gateway",
    "checkout page",
    "shopping cart",
    "email templates",
    "PDF renderer",
    "CSV import",
    "sync engine",
    "offline mode",
    "dark theme",
    "keyboard shortcuts",
    "settings page",
    "onboarding wizard",
    "access control",
    "retry policy",
    "blob storage",
    "Kubernetes deployment",
    "Helm chart",
    "Terraform module",
    "database migration",
    "cache invalidation",
    "telemetry pipeline",
    "localization",
    "session timeout",
    "password reset",
    "OAuth consent",
    "service bus consumer",
    "reporting job",
];

const SYMPTOMS: &[&str] = &[
    "fails on",
    "times out during",
    "crashes after",
    "returns 500 for",
    "shows stale data after",
    "is slow with",
    "leaks memory during",
    "double-submits on",
    "ignores",
    "breaks after",
    "throws NullReferenceException on",
    "loses state on",
];

const CONTEXTS: &[&str] = &[
    "SSO redirect",
    "large tenants",
    "Safari 17",
    "empty filter",
    "expired PAT",
    "retry",
    "Windows agents",
    "first sync",
    "network loss",
    "daylight saving change",
    "concurrent edits",
    "multi-project orgs",
    "mobile viewport",
    "high DPI displays",
    "cold start",
    "account switch",
];

const VERBS: &[&str] = &[
    "Fix",
    "Add",
    "Refactor",
    "Remove",
    "Update",
    "Migrate",
    "Improve",
    "Implement",
    "Clean up",
    "Review",
    "Document",
    "Harden",
    "Optimize",
    "Investigate",
];

const PR_SUFFIXES: &[&str] = &[
    "",
    "",
    " behind feature flag",
    " for large tenants",
    " on Windows agents",
    " (hotfix)",
    " and add tests",
    " to .NET 9",
];

const CONVENTIONAL: &[&str] = &["feat", "fix", "chore", "refactor", "perf", "docs", "test"];

const SCOPES: &[&str] = &[
    "auth", "api", "web", "ci", "infra", "billing", "search", "sync", "ui",
];

const WI_TYPES: &[&str] = &["Bug", "Bug", "Bug", "Task", "Task", "User Story", "Feature"];

const WI_STATES: &[&str] = &["New", "Active", "Active", "Resolved", "Closed"];

const ROLES: &[&str] = &[
    "admin",
    "reviewer",
    "developer",
    "release manager",
    "tenant owner",
];

const ACTIONS: &[&str] = &[
    "filter my pull requests",
    "snooze a notification",
    "export invoices as CSV",
    "reset my password",
    "see pipeline failures",
    "switch accounts",
    "pin a dashboard",
    "approve a release gate",
];

const REPO_DOMAINS: &[&str] = &[
    "payments",
    "identity",
    "web",
    "mobile",
    "infra",
    "shared",
    "docs",
    "data",
    "billing",
    "search",
    "notifications",
    "gateway",
    "reporting",
    "orders",
    "catalog",
    "inventory",
];

const REPO_SUFFIXES: &[&str] = &[
    "api",
    "portal",
    "service",
    "app",
    "terraform",
    "libs",
    "site",
    "platform",
    "worker",
    "sdk",
];

const PROJECTS: &[&str] = &[
    "Contoso Platform",
    "Fabrikam Web",
    "Northwind Payments",
    "Tailspin Mobile",
    "Woodgrove Identity",
    "Adventure Works Data",
    "Litware Infra",
    "Proseware Billing",
];

const FIRST_NAMES: &[&str] = &[
    "Alice",
    "Bob",
    "Chen",
    "Dmitri",
    "Élodie",
    "Fatima",
    "Gustavo",
    "Hana",
    "Ivan",
    "José",
    "Kenji",
    "Lena",
    "Łukasz",
    "Maya",
    "Nikolai",
    "Olga",
    "Priya",
    "Quentin",
    "Rosa",
    "Søren",
    "Tomás",
    "Uma",
    "Viktor",
    "Wen",
    "Ximena",
    "Yusuf",
    "Zoë",
    "Amara",
    "Björn",
    "Chloé",
    "Diego",
    "Emre",
    "François",
    "Greta",
    "Hiroshi",
    "Inès",
    "Jonas",
    "Kaito",
    "Lucía",
    "Mateo",
    "Noor",
    "Oskar",
    "Paulo",
    "Renée",
    "Sven",
    "Tariq",
    "Ulla",
    "Valentina",
];

const LAST_NAMES: &[&str] = &[
    "Anderson",
    "Brown",
    "Castillo",
    "Dubois",
    "Eriksson",
    "Fernández",
    "García",
    "Hansen",
    "Ivanova",
    "Jensen",
    "Kowalski",
    "Larsen",
    "Müller",
    "Nakamura",
    "O'Brien",
    "Petrov",
    "Quinn",
    "Rossi",
    "Schmidt",
    "Tanaka",
    "Ueda",
    "Vasquez",
    "Wójcik",
    "Xu",
    "Yilmaz",
    "Zimmermann",
    "Novák",
    "Lindqvist",
    "Nguyen",
    "Okafor",
    "Patel",
    "Kim",
    "Silva",
    "Moreau",
    "Haddad",
    "Kapoor",
    "Lefèvre",
    "Åberg",
    "Horváth",
    "Sørensen",
];

const COMMANDS: &[(&str, &str)] = &[
    ("Open settings", "preferences config"),
    ("Toggle theme", "dark light appearance"),
    ("Switch organization", "org account"),
    ("Sync now", "refresh pull fetch"),
    ("Open notification center", "inbox alerts"),
    ("Snooze notification", "later remind"),
    ("Mark all notifications read", "inbox clear"),
    ("Go to my pull requests", "prs review"),
    ("Go to my work items", "tasks bugs"),
    ("Go to pipelines", "builds ci"),
    ("Copy web link", "url clipboard"),
    ("Copy branch name", "git clipboard"),
    ("Open in browser", "web external"),
    ("Approve pull request", "vote approve"),
    ("Request changes", "vote wait"),
    ("Complete pull request", "merge finish"),
    ("Toggle reduce motion", "accessibility animation"),
    ("Toggle sounds", "audio mute"),
    ("Change background", "wallpaper appearance"),
    ("Reload window", "restart"),
    ("Show keyboard shortcuts", "keymap help"),
    ("Sign out", "logout account"),
    ("Report an issue", "bug feedback"),
    ("Customize palette rows", "display fields"),
    ("Open command palette", "search"),
    ("Quit adoken", "exit close"),
];

/// Deterministic corpus of `n` docs shaped like one ADO org.
pub fn corpus(n: usize, seed: u64) -> Vec<SearchDoc> {
    let mut rng = Rng(seed);
    let people = people_names(&mut rng, 256);
    let repos = repo_names(64);

    let n_wi = n * 60 / 100;
    let n_pr = n * 20 / 100;
    let n_pipe = n * 5 / 100;
    let n_person = n * 10 / 100;
    let n_misc = n - n_wi - n_pr - n_pipe - n_person;

    let mut kinds = Vec::with_capacity(n);
    kinds.extend(std::iter::repeat_n(DocKind::WorkItem, n_wi));
    kinds.extend(std::iter::repeat_n(DocKind::PullRequest, n_pr));
    kinds.extend(std::iter::repeat_n(DocKind::Pipeline, n_pipe));
    kinds.extend(std::iter::repeat_n(DocKind::Person, n_person));
    let n_cmd = n_misc.min(COMMANDS.len());
    let n_proj = (n_misc - n_cmd).min(PROJECTS.len());
    kinds.extend(std::iter::repeat_n(DocKind::Command, n_cmd));
    kinds.extend(std::iter::repeat_n(DocKind::Project, n_proj));
    kinds.extend(std::iter::repeat_n(DocKind::Repo, n_misc - n_cmd - n_proj));
    // Fisher-Yates: stores return mixed kinds, not grouped.
    for i in (1..kinds.len()).rev() {
        kinds.swap(i, rng.below(i + 1));
    }

    let (mut wi_id, mut pr_id, mut pipe_id) = (1000u64, 100u64, 1u64);
    let (mut person, mut command, mut project, mut repo) = (0u64, 0u64, 0u64, 0u64);
    kinds
        .into_iter()
        .map(|kind| match kind {
            DocKind::WorkItem => {
                wi_id += 1 + rng.below(3) as u64;
                work_item(&mut rng, wi_id, &people)
            }
            DocKind::PullRequest => {
                pr_id += 1 + rng.below(2) as u64;
                pull_request(&mut rng, pr_id, wi_id, &people, &repos)
            }
            DocKind::Pipeline => {
                pipe_id += 1;
                pipeline(&mut rng, pipe_id, &repos)
            }
            DocKind::Person => {
                person += 1;
                let name = &people[rng.below(people.len())];
                SearchDoc {
                    id: person,
                    kind,
                    title: name.clone(),
                    subtitle: email(name),
                }
            }
            DocKind::Command => {
                let (name, keywords) = COMMANDS[command as usize];
                command += 1;
                SearchDoc {
                    id: command,
                    kind,
                    title: name.to_owned(),
                    subtitle: keywords.to_owned(),
                }
            }
            DocKind::Project => {
                let name = PROJECTS[project as usize];
                project += 1;
                SearchDoc {
                    id: project,
                    kind,
                    title: name.to_owned(),
                    subtitle: format!("{} repos", 4 + rng.below(40)),
                }
            }
            DocKind::Repo => {
                repo += 1;
                let base = &repos[repo as usize % repos.len()];
                let title = if (repo as usize) <= repos.len() {
                    base.clone()
                } else {
                    format!("{base}-{}", repo as usize / repos.len())
                };
                SearchDoc {
                    id: repo,
                    kind,
                    title,
                    subtitle: rng.pick(PROJECTS).to_owned(),
                }
            }
        })
        .collect()
}

fn people_names(rng: &mut Rng, n: usize) -> Vec<String> {
    (0..n)
        .map(|_| format!("{} {}", rng.pick(FIRST_NAMES), rng.pick(LAST_NAMES)))
        .collect()
}

fn repo_names(n: usize) -> Vec<String> {
    (0..n)
        .map(|i| {
            let domain = REPO_DOMAINS[i % REPO_DOMAINS.len()];
            let suffix = REPO_SUFFIXES[(i * 7 + i / REPO_DOMAINS.len()) % REPO_SUFFIXES.len()];
            format!("{domain}-{suffix}")
        })
        .collect()
}

fn email(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 16);
    for c in name.chars().map(crate::normalize::fold) {
        match c {
            ' ' => out.push('.'),
            c if c.is_ascii_alphanumeric() => out.push(c),
            _ => {}
        }
    }
    out.push_str("@contoso.com");
    out
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn work_item(rng: &mut Rng, id: u64, people: &[String]) -> SearchDoc {
    let ty = rng.pick(WI_TYPES);
    let component = rng.pick(COMPONENTS);
    let text = match ty {
        "Bug" => format!(
            "{} {} {}",
            capitalize(component),
            rng.pick(SYMPTOMS),
            rng.pick(CONTEXTS)
        ),
        "Task" => format!("{} {}", rng.pick(VERBS), component),
        "User Story" => format!("As a {}, I can {}", rng.pick(ROLES), rng.pick(ACTIONS)),
        _ => format!("{} redesign", capitalize(component)),
    };
    let assignee = &people[rng.below(people.len())];
    SearchDoc {
        id,
        kind: DocKind::WorkItem,
        title: format!("{ty} {id}: {text}"),
        subtitle: format!(
            "{} · {} · {}",
            rng.pick(WI_STATES),
            assignee,
            rng.pick(PROJECTS)
        ),
    }
}

fn pull_request(
    rng: &mut Rng,
    id: u64,
    latest_wi: u64,
    people: &[String],
    repos: &[String],
) -> SearchDoc {
    let component = rng.pick(COMPONENTS);
    let title = if rng.chance(25) {
        format!(
            "{}({}): {} {}",
            rng.pick(CONVENTIONAL),
            rng.pick(SCOPES),
            rng.pick(VERBS).to_lowercase(),
            component
        )
    } else if rng.chance(10) {
        format!("Merged PR {}: {} {}", id, rng.pick(VERBS), component)
    } else {
        let mut t = format!("{} {}{}", rng.pick(VERBS), component, rng.pick(PR_SUFFIXES));
        if rng.chance(30) && latest_wi > 1000 {
            t.push_str(&format!(
                " (AB#{})",
                1000 + rng.below((latest_wi - 999) as usize)
            ));
        }
        t
    };
    let repo = &repos[rng.below(repos.len())];
    let author = &people[rng.below(people.len())];
    let branch = component.to_lowercase().replace(' ', "-");
    SearchDoc {
        id,
        kind: DocKind::PullRequest,
        title,
        subtitle: format!("!{id} · {repo} · {author} · feature/{branch} → main"),
    }
}

fn pipeline(rng: &mut Rng, id: u64, repos: &[String]) -> SearchDoc {
    let repo = &repos[rng.below(repos.len())];
    let title = match rng.below(4) {
        0 => format!("{repo}-ci"),
        1 => format!("{repo}-release"),
        2 => format!("Nightly {} tests", rng.pick(COMPONENTS)),
        _ => format!("{repo} PR validation"),
    };
    SearchDoc {
        id,
        kind: DocKind::Pipeline,
        title,
        subtitle: format!(
            "\\{}\\{} · #20260925.{}",
            rng.pick(SCOPES),
            repo,
            1 + rng.below(40)
        ),
    }
}

/// Words users type: title vocabulary split into words.
fn vocabulary() -> Vec<String> {
    let mut words: Vec<String> = COMPONENTS
        .iter()
        .chain(CONTEXTS)
        .chain(VERBS)
        .chain(REPO_DOMAINS)
        .chain(FIRST_NAMES)
        .chain(LAST_NAMES)
        .flat_map(|s| s.split(|c: char| !c.is_alphanumeric()))
        .filter(|w| w.chars().count() >= 3)
        .map(str::to_lowercase)
        .collect();
    words.sort_unstable();
    words.dedup();
    words
}

/// Swaps two adjacent, different chars: one OSA edit.
fn transpose(rng: &mut Rng, word: &str) -> Option<String> {
    let mut chars: Vec<char> = word.chars().collect();
    let spots: Vec<usize> = (0..chars.len().saturating_sub(1))
        .filter(|&i| chars[i] != chars[i + 1])
        .collect();
    if chars.len() < 4 || spots.is_empty() {
        return None;
    }
    let i = spots[rng.below(spots.len())];
    chars.swap(i, i + 1);
    Some(chars.into_iter().collect())
}

/// Recorded-style query set for AC-1: `n` deterministic queries over the
/// corpus vocabulary. At least every 8th query carries an adjacent
/// transposition (≥ 50 of 500), plus prefixes, multi-word, substitution
/// typos, people and ids.
pub fn queries(n: usize, seed: u64) -> Vec<String> {
    let mut rng = Rng(seed ^ 0x5EA2_C4ED);
    let vocab = vocabulary();
    let long: Vec<&String> = vocab.iter().filter(|w| w.chars().count() >= 4).collect();
    let pick = |rng: &mut Rng| vocab[rng.below(vocab.len())].clone();
    let pick_long = |rng: &mut Rng| long[rng.below(long.len())].clone();
    (0..n)
        .map(|i| {
            if i % 8 == 0 {
                loop {
                    let word = pick_long(&mut rng);
                    if let Some(t) = transpose(&mut rng, &word) {
                        break if rng.chance(40) {
                            format!("{t} {}", pick(&mut rng))
                        } else {
                            t
                        };
                    }
                }
            } else {
                match rng.below(100) {
                    0..=24 => pick(&mut rng),
                    25..=44 => {
                        let w = pick(&mut rng);
                        let len = 2 + rng.below(4);
                        w.chars().take(len).collect()
                    }
                    45..=64 => {
                        let c = rng.pick(COMPONENTS).to_lowercase();
                        if c.contains(' ') {
                            c
                        } else {
                            format!("{} {c}", rng.pick(VERBS).to_lowercase())
                        }
                    }
                    65..=72 => format!(
                        "{} {} {}",
                        rng.pick(VERBS).to_lowercase(),
                        pick(&mut rng),
                        pick(&mut rng)
                    ),
                    73..=82 => {
                        let mut chars: Vec<char> = pick_long(&mut rng).chars().collect();
                        let at = 1 + rng.below(chars.len() - 1);
                        chars[at] = (b'a' + rng.below(26) as u8) as char;
                        chars.into_iter().collect()
                    }
                    83..=91 => {
                        let id = 1000 + rng.below(60_000);
                        let digits = 3 + rng.below(3);
                        id.to_string().chars().take(digits).collect()
                    }
                    _ => format!(
                        "{} {}",
                        rng.pick(FIRST_NAMES).to_lowercase(),
                        rng.pick(LAST_NAMES)
                            .to_lowercase()
                            .chars()
                            .take(3)
                            .collect::<String>()
                    ),
                }
            }
        })
        .collect()
}
