# Release: macOS bundle, signing, notarization

How `adoken.app` is built, and the steps that turn a local ad hoc bundle into
one users can download. Everything under "Distribution" is a checklist for the
release-manager; none of it runs in development or CI yet.

## Local bundle (development)

```sh
scripts/bundle.sh             # debug build → target/bundle/adoken.app
scripts/bundle.sh --release   # release profile (thin LTO, stripped symbols)
scripts/bundle.sh --no-build  # re-bundle the existing binary
scripts/bundle.sh --run       # build, bundle, quit the running adoken, open the new one
scripts/bundle.sh --adhoc     # ad hoc signature (CI does this automatically)
```

The script:

1. Builds `adoken` (`cargo build -p adoken --bin adoken`, or `--release`).
2. Assembles `target/bundle/adoken.app`:
   - `Contents/MacOS/adoken`: the binary.
   - `Contents/Info.plist`: from `crates/app/bundle/Info.plist`, with
     `CFBundleShortVersionString` = workspace version and `CFBundleVersion`
     = commit count (it must only grow).
   - `Contents/Resources/adoken.icns`: `sips` scales
     `crates/app/bundle/icon-1024.png` to every iconset size and `iconutil`
     packs them.
   - `Contents/Resources/licenses/`: `THIRD_PARTY_NOTICES.md` and the Lucide
     license, which must ship with every binary.
3. Signs it with the local "adoken dev" identity and verifies the signature,
   then re-registers the bundle with LaunchServices so the icon and bundle id
   show up at once. The identity comes from `scripts/dev-signing.sh`, which
   `bundle.sh` runs automatically: a self-signed code-signing certificate in
   a private keychain (`~/Library/Keychains/adoken-dev.keychain-db`, password
   in `~/.config/adoken/dev-keychain-password`, 0600) that unlocks without
   prompts. Trusting it for code signing asks for your macOS password once,
   the first time only. Why: the Keychain remembers "Always Allow" per code
   requirement. An ad hoc signature's requirement is the binary's hash, so
   every rebuild asked for the Keychain password again. The certificate-based
   requirement (`identifier "dev.adoken.app" and certificate leaf = …`) stays
   the same across rebuilds: click "Always Allow" once and it sticks.
   `scripts/dev-signing.sh --remove` deletes the keychain, trust and password.
   With `CI` set or `--adhoc`, it signs ad hoc (`codesign --sign -`).

Run it:

```sh
open target/bundle/adoken.app                           # as from Finder
open --env ADOKEN_DEMO=1 target/bundle/adoken.app       # fake Contoso org
ADOKEN_DEMO=1 target/bundle/adoken.app/Contents/MacOS/adoken   # logs in the terminal
```

Both launch styles count as "inside the bundle": `NSBundle.mainBundle` has
the bundle id `dev.adoken.app`, so OS banners and the permission prompt work.
A plain `cargo run` has no bundle id. Banners are then skipped (the inbox
shows in-app toasts only) and Settings shows the permission as
"Unavailable"; the dock badge still works.

### Bundle identity

| Key | Value | Why |
| --- | --- | --- |
| `CFBundleIdentifier` | `dev.adoken.app` | Notification Center, TCC and LaunchServices key on it. Never change it after the first public release. |
| `LSUIElement` | `false` | A regular dock app: the dock badge carries the unread P0+P1 count (spec 0005 flow step 6). |
| `NSUserNotificationsUsageDescription` | purpose string | Explains banners. macOS only prompts when the user turns banners on in Settings or when the first banner is due. |
| `LSMinimumSystemVersion` | `12.0` | gpui's Metal renderer. `UNUserNotificationCenter` needs 10.14. |

### App icon

`crates/app/bundle/icon-1024.png` is the master. It is the original adoken
pixel fist (`public/src/art/fist.ts`, the same art as the site favicon) on a
dark tile with an impact burst. The colors come from `public/src/theme.ts`,
and it follows Apple's macOS icon grid: an 824 px tile inset 100 px, with a
baked-in shadow, because `sips` can't mask. Regenerate it after changing the
fist or the palette:

```sh
(cd public && bun install)
bun crates/app/bundle/render-icon.ts
```

The PNG is committed, so bundling needs only the tools that ship with macOS
(`sips`, `iconutil`, `codesign`, `plutil`).

## Sharing a test build

```sh
scripts/dmg.sh              # release bundle → target/bundle/adoken-<version>.dmg
scripts/dmg.sh --no-build   # package the existing target/bundle/adoken.app
```

The dmg holds `adoken.app` and an `Applications` link (drag one onto the
other). Its signature is local ("adoken dev" or ad hoc), so on any other Mac
Gatekeeper refuses the first launch ("Apple could not verify…"). The
recipient allows it once, either:

- open the app, dismiss the warning, then System Settings → Privacy &
  Security → "Open Anyway" (macOS 15+ dropped right-click → Open), or
- `xattr -dr com.apple.quarantine /Applications/adoken.app`.

The binary is arm64 only: Apple silicon Macs. Use this for trusted testers
only; public downloads go through "Distribution" below.

## Distribution (not executed yet)

An ad hoc signature is fine on the machine that built the app. For anyone
else, Gatekeeper requires a Developer ID signature and an Apple notarization
ticket. Checklist per release (see `.omp/agents/release-manager.md`):

### 1. One-time setup

- Apple Developer Program membership (the account holder creates the
  certificate).
- A **Developer ID Application** certificate in the release machine's login
  keychain. Check it with
  `security find-identity -v -p codesigning` (it should list
  `"Developer ID Application: <Name> (<TEAMID>)"`).
- An App Store Connect API key for `notarytool` (Users and Access →
  Integrations → Keys, role *Developer*). Keep the `.p8` file out of the repo.
  Record its Key ID and Issuer ID.
- Optionally, store the credentials once in the keychain:
  `xcrun notarytool store-credentials adoken-notary --key <path.p8> --key-id <KEY_ID> --issuer <ISSUER_ID>`.

### 2. Build and sign

```sh
scripts/bundle.sh --release
codesign --force --options runtime --timestamp \
  --sign "Developer ID Application: <Name> (<TEAMID>)" \
  target/bundle/adoken.app
codesign --verify --strict --verbose=2 target/bundle/adoken.app
```

- `--options runtime` (hardened runtime) and `--timestamp` are both required
  for notarization.
- No `--deep`: Apple deprecated it, and the bundle is a single Mach-O with no
  nested code.
- Entitlements: none needed today. The app makes outbound HTTPS calls
  (allowed by default without the App Sandbox), reads the Keychain item it
  created, and posts user notifications. If the App Sandbox is ever turned on,
  add `com.apple.security.network.client` and re-test Keychain access.

### 3. Notarize and staple

```sh
ditto -c -k --keepParent target/bundle/adoken.app target/bundle/adoken.zip
xcrun notarytool submit target/bundle/adoken.zip --keychain-profile adoken-notary --wait
xcrun stapler staple target/bundle/adoken.app
spctl --assess --type execute --verbose target/bundle/adoken.app   # "source=Notarized Developer ID"
```

If the submission is rejected, read the log with
`xcrun notarytool log <submission-id> --keychain-profile adoken-notary`.
Staple the `.app` before packaging it so it passes Gatekeeper offline.

### 4. Package

- A `.dmg` with the classic "drag into Applications" layout (for example
  with `dmgbuild`). Sign the dmg with the same identity, notarize it, and
  staple it too (`notarytool submit adoken.dmg …`, then
  `stapler staple adoken.dmg`).
- Before the first public binary, include the generated license bundle
  required by `THIRD_PARTY_NOTICES.md` ("Other crates"), plus the Apache-2.0
  text and zui's `NOTICE`.

### 5. Verify on a clean machine

- Download the dmg through a browser (so it gets the quarantine attribute)
  and open it. There should be no Gatekeeper warning beyond the standard
  "downloaded from the internet" prompt.
- Turn on "Inbox + banner" for P1 in Settings → Notifications. Check that
  macOS prompts once, that a mention shows a banner grouped under its pull
  request, that clicking it opens adoken on that pull request, and that the
  dock badge matches the unread P0+P1 count.

### Secrets

Signing identities, the `.p8` key and notary profiles live only in the
release machine's keychain or CI secrets. Never commit them, and never pass
them on a command line that gets logged.
