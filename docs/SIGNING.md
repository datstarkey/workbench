# Code signing & notarization (macOS)

Workbench ships as a Developer ID–signed, notarized app. Without this, macOS
Gatekeeper refuses to open the download ("Apple could not verify…") and
`UNUserNotificationCenter` refuses to register the app at all — which is why
`send_fallback_notification` exists as an `osascript` workaround.

Signing is **opt-in by environment**: `bun run dev` and `bun run build` stay
unsigned. Only tagged CI releases and `bun run build:signed` sign.

## What produces what

| Artifact                         | Signed                               | Notarized                                                    |
| -------------------------------- | ------------------------------------ | ------------------------------------------------------------ |
| `Workbench.app`                  | yes, hardened runtime + entitlements | yes, stapled                                                 |
| `Workbench_x.y.z_*.dmg`          | yes                                  | not separately — the stapled app inside satisfies Gatekeeper |
| `Workbench.app.tar.gz` (updater) | derived from the signed app          | inherits the staple                                          |

Windows is **not** signed yet — the NSIS installer still trips SmartScreen.

## One-time setup

### 1. Developer ID Application certificate

Fastest route is Xcode, which generates the private key and CSR for you:

`Xcode → Settings → Accounts → <your Apple ID> → Manage Certificates → + → Developer ID Application`

Verify it landed:

```sh
security find-identity -v -p codesigning
# 1) ABC123… "Developer ID Application: Your Name (TEAMID1234)"
```

The quoted string is `APPLE_SIGNING_IDENTITY`. The parenthesised suffix is your Team ID.

> Keep the private key. If you lose it, the certificate is dead — you must revoke
> and reissue, and Apple caps you at 5 Developer ID certificates per account.

### 2. App Store Connect API key (notarization)

https://appstoreconnect.apple.com/access/integrations/api → **Keys** → **+**

- Access: **Developer** is sufficient.
- Download the `.p8` immediately — Apple allows exactly one download.
- Record the **Key ID** (`APPLE_API_KEY`) and the **Issuer ID** shown above the table (`APPLE_API_ISSUER`).

Park it where the Tauri CLI looks by default:

```sh
mkdir -p ~/.appstoreconnect/private_keys
mv ~/Downloads/AuthKey_*.p8 ~/.appstoreconnect/private_keys/
chmod 600 ~/.appstoreconnect/private_keys/*.p8
```

### 3. Local builds

```sh
cp signing.env.example signing.env   # gitignored
$EDITOR signing.env
bun run --cwd apps/desktop build:signed
```

Notarization adds 2–10 minutes to the build. Verify the result:

```sh
APP="apps/desktop/src-tauri/target/universal-apple-darwin/release/bundle/macos/Workbench.app"
codesign -dv --verbose=4 "$APP"        # expect: Authority=Developer ID Application, flags=…(runtime)
spctl -a -vvv -t install "$APP"        # expect: accepted / source=Notarized Developer ID
xcrun stapler validate "$APP"          # expect: The validate action worked!
```

### 4. GitHub Actions secrets

Six secrets drive `.github/workflows/release.yml`. Set them from the CLI —
export the certificate to a `.p12` first (Keychain Access → right-click the
**private key** under "Developer ID Application" → Export → `.p12`, set a password).

```sh
gh secret set APPLE_CERTIFICATE < <(base64 -i ~/Downloads/Certificates.p12 | tr -d '\n')
gh secret set APPLE_CERTIFICATE_PASSWORD          # the .p12 export password
gh secret set APPLE_SIGNING_IDENTITY              # "Developer ID Application: … (TEAMID1234)"
gh secret set APPLE_API_KEY                       # Key ID
gh secret set APPLE_API_ISSUER                    # Issuer ID
gh secret set APPLE_API_KEY_P8 < <(base64 -i ~/.appstoreconnect/private_keys/AuthKey_XXXX.p8 | tr -d '\n')
```

The `tr -d '\n'` matters — macOS `base64` wraps at 76 columns and the workflow
decodes with `base64 --decode`, which tolerates it, but a single line keeps the
secret diff-clean.

Delete the `.p12` afterwards; it contains the exportable private key.

The workflow degrades rather than fails: no `APPLE_CERTIFICATE` → unsigned build
with a CI warning; certificate but no `APPLE_API_KEY_P8` → signed but un-notarized.

## Entitlements

`apps/desktop/src-tauri/Entitlements.plist` is deliberately near-empty. The app is
not sandboxed, so spawning shells/PTYs, reading arbitrary project directories and
binding the control-plane port need no entitlement. Swift is statically linked
(`build.rs`), so library validation stays on.

The one entitlement — `com.apple.security.automation.apple-events` — exists because
hardened runtime otherwise blocks the `osascript` calls in `commands.rs`.

Add entitlements only with a demonstrated failure behind them; each one is a
weakening of the hardened runtime and Apple reviews them for notarization.

## Gotchas

- **Bundle identifier is load-bearing.** `com.starkeydigital.workbench` is baked into
  the signature, TCC permission grants and the updater's app identity. Changing it
  resets every macOS permission the user has granted.
- **Half-set Apple ID credentials hard-fail.** The Tauri bundler errors if `APPLE_ID`
  and `APPLE_PASSWORD` are set without `APPLE_TEAM_ID`. We use the API-key path
  instead, which has no such trap.
- **`minimumSystemVersion` is 10.15**, not the Tauri scaffold's 10.13 — Tauri v2 does
  not support anything older, so the old value was a claim the binary couldn't honour.
- Certificates expire after 5 years; notarization of already-stapled builds keeps
  working after expiry, but new builds do not.
