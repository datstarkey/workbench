# Code signing & notarization (macOS)

Workbench ships as a Developer ID–signed, notarized app. Without this, macOS
Gatekeeper refuses to open the download ("Apple could not verify…") and
`UNUserNotificationCenter` refuses to register the app at all — which is why
`send_fallback_notification` exists as an `osascript` workaround.

Signing is **opt-in by environment**: `bun run dev` and `bun run build` stay
unsigned. Only tagged CI releases and `bun run build:signed` sign.

## What produces what

| Artifact                         | Signed                               | Notarized                                    |
| -------------------------------- | ------------------------------------ | -------------------------------------------- |
| `Workbench.app`                  | yes, hardened runtime + entitlements | yes, stapled                                 |
| `Workbench_x.y.z_*.dmg`          | yes                                  | yes, stapled — as a separate step, see below |
| `Workbench.app.tar.gz` (updater) | derived from the signed app          | inherits the staple                          |

Windows is **not** signed yet — the NSIS installer still trips SmartScreen.

## One-time setup

### 1. Developer ID Application certificate

Fastest route is Xcode, which generates the private key and CSR for you:

`Xcode → Settings → Accounts → <your Apple ID> → Manage Certificates → + → Developer ID Application`

Verify it landed:

```sh
security find-identity -v -p codesigning
# 1) ABC123… "Developer ID Application: Starkey Digital ltd (2MFPW59LV6)"
```

The quoted string is `APPLE_SIGNING_IDENTITY`; `2MFPW59LV6` is the Team ID.

If Xcode reports **"failed to retrieve teams"**, see [Troubleshooting](#troubleshooting) —
the web portal route below works even when Xcode's team fetch is broken.

<details>
<summary>Manual route (no Xcode)</summary>

```sh
openssl req -new -newkey rsa:2048 -nodes \
  -keyout ~/Desktop/developerID.key \
  -out ~/Desktop/developerID.certSigningRequest \
  -subj "/emailAddress=jake.starkey@starkeydigital.com/CN=Starkey Digital ltd/C=GB"
```

Upload the `.certSigningRequest` at
https://developer.apple.com/account/resources/certificates/add → **Developer ID Application**,
download the resulting `.cer`, then combine key + cert into the keychain:

```sh
openssl x509 -in ~/Downloads/developerID_application.cer -inform DER -out /tmp/devid.pem -outform PEM
openssl pkcs12 -export -inkey ~/Desktop/developerID.key -in /tmp/devid.pem -out ~/Desktop/developerID.p12
security import ~/Desktop/developerID.p12 -k ~/Library/Keychains/login.keychain-db
```

That `.p12` is also exactly what `APPLE_CERTIFICATE` wants, so this route skips the
Keychain Access export in §4.

</details>

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
APP="target/universal-apple-darwin/release/bundle/macos/Workbench.app"
codesign -dv --verbose=4 "$APP"        # expect: Authority=Developer ID Application, flags=…(runtime)
spctl -a -vvv -t install "$APP"        # expect: accepted / source=Notarized Developer ID
xcrun stapler validate "$APP"          # expect: The validate action worked!
```

### 4. GitHub Actions secrets

Six secrets drive `.github/workflows/release.yml`. Set them from the CLI —
export the certificate to a `.p12` first (Keychain Access → right-click the
**private key** under "Developer ID Application" → Export → `.p12`, set a password).

Identifiers — safe to pass inline:

```sh
gh secret set APPLE_SIGNING_IDENTITY --body 'Developer ID Application: Starkey Digital ltd (2MFPW59LV6)'
gh secret set APPLE_API_KEY          --body 'XSL34F6XM9'
gh secret set APPLE_API_ISSUER       --body '8c8343a9-5575-4f63-8757-d65bc0f8491f'
```

Key material — piped or prompted, never inline, so it stays out of shell history:

```sh
base64 -i ~/Desktop/developerID.p12 | tr -d '\n' | gh secret set APPLE_CERTIFICATE
gh secret set APPLE_CERTIFICATE_PASSWORD          # prompts; the .p12 export password
base64 -i ~/.appstoreconnect/private_keys/AuthKey_XSL34F6XM9.p8 | tr -d '\n' | gh secret set APPLE_API_KEY_P8
```

The `tr -d '\n'` matters — macOS `base64` wraps at 76 columns. The workflow decodes
with `base64 --decode`, which tolerates wrapping, but a single line keeps the secret
diff-clean.

`rm ~/Desktop/developerID.p12` afterwards; it holds the exportable private key.

> **A secret can exist but be empty.** `gh secret set NAME` without `--body` reads the
> value from stdin, and with no TTY attached (a script, a non-interactive shell) it
> stores an empty string. `gh secret list` shows it as present either way. This shipped
> v0.27.0 publicly unsigned. Always use `--body` or a pipe, never the bare prompting
> form, and confirm the CI log shows `***` rather than a blank after the variable name.

The workflow **fails** if any of the six is empty or unset — it only ever runs on a
`v*` tag, so every run is a release and an unsigned artifact is never the right answer.
To ship unsigned deliberately (expired certificate, Apple outage), set the repository
variable `ALLOW_UNSIGNED_RELEASE=true`.

## Why the DMG is notarized separately

Tauri notarizes and staples the `.app`, then builds the `.dmg` and only **signs** it —
it never submits the DMG. But Gatekeeper assesses the container the user actually
downloads, so that isn't enough:

```
$ spctl -a -t open --context context:primary-signature Workbench_0.2.0_universal.dmg
rejected                              # ← before
source=Unnotarized Developer ID
```

Both `build-signed.sh` and the release workflow therefore run `notarytool submit` +
`stapler staple` against the DMG after the Tauri build, which flips it to
`accepted / source=Notarized Developer ID`.

In CI this has to happen _after_ `tauri-action`, which has already uploaded the
unstapled DMG — so the step re-uploads with `gh release upload --clobber`. That is
safe only because the release stays a **draft** until the `publish` job. If the draft
behaviour ever changes, this step must move before the upload instead.

The updater tarball needs no equivalent step: it wraps the already-stapled `.app`, and
the updater replaces the bundle in place.

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

## Troubleshooting

**Xcode: "failed to retrieve teams" / no teams listed.** In rough order of likelihood:

1. **Membership activated or renewed within the last few hours.** Apple's provisioning
   backend propagates separately from the billing system that sends the confirmation
   email, so Xcode can see an empty team list for a while after activation. Wait, then
   `Xcode → Settings → Accounts → −` and re-add the Apple ID (a restart alone often
   isn't enough — the empty team list is cached).
2. **Unaccepted Program License Agreement.** A renewal frequently ships a new PLA, and
   until it's accepted the API Xcode calls returns nothing rather than an error. Sign in
   at https://developer.apple.com/account — a banner appears if one is pending. This is
   the most common cause that _doesn't_ resolve on its own.
3. **Stale auth session.** Sign out of Xcode's Accounts pane entirely, quit Xcode, sign
   back in.

The web portal is authoritative: if https://developer.apple.com/account/resources/certificates
lists your team and lets you add a certificate, the membership is live and the problem is
local to Xcode — use the manual CSR route in §1 and ignore Xcode entirely.

**Team ID:** `2MFPW59LV6` (Starkey Digital ltd). Not a secret — it's embedded in the
signature of every build you ship.
