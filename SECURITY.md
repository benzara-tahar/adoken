# Security policy

adoken holds a Personal Access Token for your Azure DevOps organization, so security reports are taken seriously.

## Reporting a vulnerability

Please **don't open a public issue**. Report it privately through GitHub:
**Security › Report a vulnerability** on this repository. Include what you found, how to reproduce it, and what
an attacker could do with it.

You'll get an acknowledgement within a week. Fixes ship in the next release and are credited in the changelog
unless you'd rather stay anonymous.

## Scope

In scope: token handling (Keychain storage, requested scopes, anything that could write the token to disk, logs,
crash reports or the network log), the local SQLite store, the Azure DevOps client, and the macOS bundle and
signing scripts.

Out of scope: vulnerabilities in Azure DevOps itself (report those to Microsoft) and issues that need an attacker
who already controls your user account.

## Supported versions

adoken is pre-1.0. Only the latest release gets security fixes.
