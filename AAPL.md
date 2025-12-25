# TODO

## Apple Notarization Fix (macOS CI)

**Problem**: macOS build fails with 401 "Invalid credentials" during notarization.

**Cause**: App-specific password expired or was revoked.

**Fix**:

1. Go to https://appleid.apple.com
2. Sign-In and Security → App-Specific Passwords
3. Generate new password (name it e.g. "playa-ci")
4. Go to GitHub repo → Settings → Secrets and variables → Actions
5. Update secret `APPLE_PASSWORD` with new app-specific password
6. Re-run failed workflow

**Secrets used**:
- `APPLE_ID` - Apple ID email
- `APPLE_PASSWORD` - App-specific password ← UPDATE THIS
- `APPLE_TEAM_ID` - Team ID
- `APPLE_CERTIFICATE` - Base64 encoded .p12
- `APPLE_CERTIFICATE_PASSWORD` - Certificate password
