# macOS Notarization Setup

This document explains how to set up Apple notarization for macOS builds.

## Problem

Currently, Playa is **code-signed** but **NOT notarized**:
- ✅ **Code signing** - App is signed with Developer ID certificate (proves authenticity)
- ❌ **Notarization** - App is NOT submitted to Apple for malware scanning
- ⚠️ **Result** - Users see "Apple could not verify" warning when installing from DMG

## Solution

Enable notarization by configuring Apple credentials in GitHub Actions.

## Prerequisites

1. **Apple Developer Account** - [developer.apple.com](https://developer.apple.com)
2. **Developer ID Certificate** - Already configured (APPLE_CERTIFICATE secret exists)
3. **Notarization Credentials** - Need to be added (see below)

## Setup Instructions

### Method 1: App Store Connect API (Recommended for CI/CD)

This method is more secure and doesn't require 2FA.

#### Step 1: Create API Key

1. Go to [App Store Connect](https://appstoreconnect.apple.com)
2. Navigate to: **Users and Access** → **Integrations** → **App Store Connect API**
3. Click **Generate API Key** or use existing key
4. Select **Access**: **App Manager** or **Developer** role
5. Download the `.p8` file (you can only download it once!)
6. Note the **Key ID** (e.g., `ABC1234567`)
7. Note the **Issuer ID** (e.g., `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`)

#### Step 2: Add GitHub Secrets

Go to repository settings: https://github.com/ssoj13/playa/settings/secrets/actions

Add these secrets:

| Secret Name | Value | Description |
|-------------|-------|-------------|
| `APPLE_API_KEY` | Your Key ID | Example: `ABC1234567` |
| `APPLE_API_ISSUER` | Your Issuer ID | Example: `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx` |
| `APPLE_API_KEY_PATH` | Base64-encoded .p8 file | See below for encoding |

**Encoding the .p8 file:**
```bash
# macOS/Linux
base64 -i AuthKey_ABC1234567.p8 | pbcopy  # macOS (copies to clipboard)
base64 -i AuthKey_ABC1234567.p8            # Linux (prints to terminal)

# Windows PowerShell
[Convert]::ToBase64String([IO.File]::ReadAllBytes("AuthKey_ABC1234567.p8"))
```

Paste the base64 string into the `APPLE_API_KEY_PATH` secret.

### Method 2: Apple ID Credentials (Simpler but less secure)

#### Step 1: Generate App-Specific Password

1. Go to [appleid.apple.com](https://appleid.apple.com)
2. Sign in with your Apple ID
3. Navigate to **Security** → **App-Specific Passwords**
4. Click **Generate an app-specific password**
5. Enter label: "GitHub Actions - Playa"
6. Save the generated password (e.g., `xxxx-xxxx-xxxx-xxxx`)

#### Step 2: Find Team ID

1. Go to [developer.apple.com/account](https://developer.apple.com/account)
2. Click on your account name (top right)
3. Note your **Team ID** (e.g., `A1B2C3D4E5`)

#### Step 3: Add GitHub Secrets

Go to repository settings: https://github.com/ssoj13/playa/settings/secrets/actions

Add these secrets:

| Secret Name | Value | Description |
|-------------|-------|-------------|
| `APPLE_ID` | Your Apple ID email | Example: `your.email@example.com` |
| `APPLE_PASSWORD` | App-specific password | Example: `xxxx-xxxx-xxxx-xxxx` |
| `APPLE_TEAM_ID` | Your Team ID | Example: `A1B2C3D4E5` (only if multiple teams) |

**Note**: `APPLE_TEAM_ID` is only required if your Apple ID belongs to multiple teams.

## Verification

After adding secrets, trigger a new build:

1. **Push a new tag**: `git tag v0.1.x && git push --tags`
2. **Monitor GitHub Actions**: Check the workflow logs
3. **Look for notarization logs**:
   ```
   [macOS] Notarization: Using App Store Connect API (Method 1)
   ```
   or
   ```
   [macOS] Notarization: Using Apple ID credentials (Method 2)
   ```

If no credentials are configured, you'll see:
```
[macOS] WARNING: No notarization credentials found!
[macOS] App will be code-signed but NOT notarized
```

## Notarization Process

When configured, cargo-packager will:

1. **Sign** the app with Developer ID certificate
2. **Create** DMG installer
3. **Submit** DMG to Apple Notary Service
4. **Wait** for Apple's malware scan (usually 1-5 minutes)
5. **Staple** the notarization ticket to the DMG

After notarization, users will see:
- ✅ "Verified by Apple" (instead of "Apple could not verify")
- ✅ No security warnings on first launch
- ✅ Clean double-click installation

## Troubleshooting

### Notarization fails with "Invalid credentials"

**Method 1 (API Key):**
- Verify `APPLE_API_KEY` matches Key ID from App Store Connect
- Verify `APPLE_API_ISSUER` is correct (UUID format)
- Verify `.p8` file is correctly base64-encoded
- Check API key has correct role: App Manager or Developer

**Method 2 (Apple ID):**
- Verify `APPLE_ID` is your full Apple ID email
- Verify `APPLE_PASSWORD` is an **app-specific password** (NOT your Apple ID password!)
- Generate a new app-specific password if needed
- If multiple teams, add `APPLE_TEAM_ID` secret

### Notarization timeout

Notarization can take 1-15 minutes depending on Apple's server load:
- Check Apple System Status: https://developer.apple.com/system-status/
- Wait and retry after a few hours
- Check workflow logs for actual error messages

### "Bundle format unrecognized"

This usually means:
- App is not properly signed before notarization
- DMG structure is incorrect
- Check that code signing step completed successfully

## Environment Variables Reference

cargo-packager supports these environment variables for macOS notarization:

### Method 1: App Store Connect API
```bash
APPLE_API_KEY          # API Key ID
APPLE_API_ISSUER       # Issuer ID (UUID)
APPLE_API_KEY_PATH     # Path or base64-encoded .p8 file
```

### Method 2: Apple ID
```bash
APPLE_ID               # Apple ID email
APPLE_PASSWORD         # App-specific password
APPLE_TEAM_ID          # Team ID (if multiple teams)
```

### Optional
```bash
APPLE_SIGNING_IDENTITY      # Override signing identity
APPLE_PROVIDER_SHORT_NAME   # Provider short name (for multiple teams)
```

## Security Best Practices

1. ✅ **Use Method 1** (API Key) for CI/CD - more secure than passwords
2. ✅ **Rotate secrets** periodically (every 6-12 months)
3. ✅ **Use GitHub Secrets** - never commit credentials to repository
4. ✅ **Limit API key scope** - only grant necessary permissions
5. ❌ **Never share** .p8 files or app-specific passwords

## Additional Resources

- [Apple Notarization Guide](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution)
- [App Store Connect API](https://developer.apple.com/documentation/appstoreconnectapi)
- [cargo-packager Documentation](https://github.com/crabnebula-dev/cargo-packager)
- [Tauri macOS Code Signing](https://v2.tauri.app/distribute/sign/macos/)

## Support

If you encounter issues:
1. Check GitHub Actions workflow logs for detailed error messages
2. Review Apple's notarization logs (cargo-packager will print them)
3. Open an issue: https://github.com/ssoj13/playa/issues
