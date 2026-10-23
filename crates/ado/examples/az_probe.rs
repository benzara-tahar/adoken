//! Read-only smoke test against this machine's real Azure CLI and Azure DevOps:
//! detect `az`, then list the organizations of the signed-in identity.
//! Prints only non-secret results. Never creates or revokes a PAT and never
//! touches the Keychain or the app data dir.
//!
//! `cargo run -p adoken-ado --example az_probe`

use std::time::Instant;

use adoken_ado::{AzStatus, Connector, MIN_AZ_VERSION};

#[tokio::main]
async fn main() {
    // A throwaway data dir: nothing is written by detect/list, but never point
    // a probe at the real one.
    let scratch = std::env::temp_dir().join("adoken-az-probe");
    let connector = Connector::new(scratch);

    let started = Instant::now();
    let status = connector.detect_az().await;
    println!("detect_az: {:.2?}", started.elapsed());
    if let Some(az) = status.az() {
        println!("  az path: {}", az.path().display());
    }
    let az = match &status {
        AzStatus::NotInstalled => return println!("  Azure CLI not found"),
        AzStatus::TooOld { found, .. } => {
            return println!("  az {found} · needs {MIN_AZ_VERSION} or newer");
        }
        AzStatus::NotLoggedIn { version, .. } => {
            return println!("  az {version} · not signed in (run `az login`)");
        }
        AzStatus::Failed { reason } => return println!("  {reason}"),
        AzStatus::Ready {
            az,
            version,
            user,
            tenant,
        } => {
            println!("  az {version} · signed in as {user} · tenant {tenant}");
            az
        }
    };

    if let Some(org) = connector.default_org(az).await {
        println!("  az devops default org: {}", org.name());
    }

    let started = Instant::now();
    let source = match connector.token_source(az).await {
        Ok(source) => source,
        Err(err) => return println!("token_source: {}", err.message()),
    };
    println!(
        "token_source (Entra token, memory only): {:.2?}",
        started.elapsed()
    );

    let started = Instant::now();
    match connector.list_orgs(&source).await {
        Ok(list) => {
            println!("list_orgs: {:.2?}", started.elapsed());
            println!("  me: {} ({})", list.me.display_name, list.me.id);
            println!("  {} organization(s):", list.orgs.len());
            for org in &list.orgs {
                println!("    - {}  dev.azure.com/{}", org.name, org.name);
            }
        }
        Err(err) => println!("list_orgs failed: {}", err.message()),
    }
    source.forget().await;
}
