//! Gallery stories of the onboarding screens (spec 0001 AC-24, AC-25). The
//! flow is driven through its real transitions with fixed results; none of
//! its effects run, so nothing touches `az`, the network or the Keychain.

use std::time::Instant;

use adoken_ado::{
    AccessLevel, AzCli, AzStatus, ConnectPath, Connector, Identity, OrgList, OrgRef,
    PasteVerification, PatMeta, PatSource, ProbeOutcome, Profile, ScopeProbe, scopes,
};
use gpui::{Context, Window};

use super::Onboarding;
use super::flow::Flow;

/// The screen a story shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Story {
    /// Screen 0 with its entrance settled.
    Welcome,
    /// 2b: create a token with `az`, every scope of the Review level.
    ReviewAccess,
    /// 2p: create a token in the browser for fabrikam.
    CreateInBrowser,
    /// 2q after a Review token verified: reads granted, the write scopes
    /// untested, Build (Read) missing.
    PasteVerify,
}

const ORG: &str = "fabrikam";

impl Onboarding {
    pub fn story(story: Story, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let now = Instant::now();
        let (mut flow, _) = Flow::start(now);
        match story {
            Story::Welcome => {}
            Story::ReviewAccess => {
                flow.detected(ready());
                flow.confirm(now);
                flow.confirm(now);
                flow.orgs_loaded(Ok(orgs()), None, now);
                flow.confirm(now);
            }
            Story::CreateInBrowser | Story::PasteVerify => {
                flow.detected(AzStatus::NotInstalled);
                flow.confirm(now);
                flow.choose_and_continue(ConnectPath::Paste);
                flow.set_paste_org(ORG);
                if story == Story::PasteVerify {
                    flow.confirm(now);
                    flow.verify();
                    flow.verified(Ok(verification()), now);
                }
            }
        }
        let connector = Connector::new(std::env::temp_dir().join("adoken-onboarding-story"));
        let mut this = Self::with_flow(connector, flow, window, cx);
        this.welcome_settled = true;
        this.revealed = usize::MAX;
        this.paste_org
            .update(cx, |input, cx| input.set_text(ORG, cx));
        // The snapshot machine's hostname must not reach the goldens.
        this.token_name = adoken_ado::display_name_for_host("miras-mbp.local").into();
        this
    }
}

fn ready() -> AzStatus {
    AzStatus::Ready {
        az: AzCli::new("/opt/homebrew/bin/az".into(), None, Default::default()),
        version: "2.83.0".into(),
        user: "mira.patel@fabrikam.com".into(),
        tenant: "fabrikam".into(),
    }
}

fn orgs() -> OrgList {
    OrgList {
        me: Profile {
            id: "me".into(),
            display_name: "Mira Patel".into(),
            email: Some("mira.patel@fabrikam.com".into()),
        },
        orgs: vec![OrgRef {
            id: "id-fabrikam".into(),
            name: ORG.into(),
        }],
    }
}

fn verification() -> PasteVerification {
    let level = AccessLevel::Review;
    let probes = scopes(level)
        .iter()
        .map(|scope| ScopeProbe {
            scope,
            outcome: match scope.id {
                "vso.code_write" | "vso.work_write" => ProbeOutcome::GrantedWriteUntested,
                "vso.build" => ProbeOutcome::Missing,
                _ => ProbeOutcome::Granted,
            },
        })
        .collect::<Vec<_>>();
    let me = Identity {
        id: "me".into(),
        display_name: "Mira Patel".into(),
    };
    let saved = PatMeta {
        org: ORG.into(),
        level,
        scopes: probes
            .iter()
            .filter(|probe| probe.outcome.is_granted())
            .map(|probe| probe.scope.id.to_owned())
            .collect(),
        untested: vec!["vso.code_write".into(), "vso.work_write".into()],
        authorization_id: None,
        valid_to: None,
        created_at: time::OffsetDateTime::UNIX_EPOCH,
        source: PatSource::Pasted,
        lifetime_days: None,
        lifetime_capped: false,
        verified: true,
        me: Some(me.clone()),
        auto_replaced: false,
        auto_replace: Default::default(),
    };
    PasteVerification {
        me,
        probes,
        saved: Some(saved),
    }
}
