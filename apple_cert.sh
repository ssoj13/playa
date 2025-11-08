#!/bin/bash
set -e
set +H  # Disable history expansion to handle special characters (!, $, etc.) in password

echo "========================================="
echo "Export Developer ID Certificate"
echo "========================================="
echo ""

# Find Developer ID certificates
echo "Looking for Developer ID Application certificates..."
CERTS=$(security find-identity -v -p codesigning | grep "Developer ID Application" || true)

if [ -z "$CERTS" ]; then
    echo "❌ No Developer ID Application certificates found in keychain"
    echo ""
    echo "========================================="
    echo "How to create Developer ID Certificate:"
    echo "========================================="
    echo ""
    echo "Step 1: Create Certificate Signing Request (CSR) in Keychain Access"
    echo "  1. Open Keychain Access (Cmd+Space → 'Keychain Access')"
    echo "  2. Menu: Keychain Access → Certificate Assistant → Request a Certificate from a Certificate Authority..."
    echo "  3. Fill in:"
    echo "     - User Email Address: your Apple Developer email"
    echo "     - Common Name: your name or company"
    echo "     - CA Email Address: LEAVE EMPTY"
    echo "     - Request is: 'Saved to disk'"
    echo "     - ✓ Let me specify key pair information"
    echo "  4. Click Continue, save as 'CertificateSigningRequest.certSigningRequest'"
    echo "  5. Key Pair: 2048 bits, RSA → Continue"
    echo ""
    echo "Step 2: Get certificate from Apple Developer Portal"
    echo "  1. Go to: https://developer.apple.com/account/resources/certificates/list"
    echo "  2. Click '+' (Create a Certificate)"
    echo "  3. Select 'Developer ID Application' → Continue"
    echo "  4. Upload your .certSigningRequest file"
    echo "  5. Download the certificate (.cer file)"
    echo "  6. Double-click the .cer file to install in Keychain"
    echo ""
    echo "Step 3: Run this script again"
    echo "  ./apple_cert.sh"
    echo ""
    exit 1
fi

echo "Found certificates:"
echo "$CERTS"
echo ""

# Extract certificate identity (SHA-1 hash or name)
CERT_SHA=$(echo "$CERTS" | head -n 1 | awk '{print $2}')
CERT_NAME=$(echo "$CERTS" | head -n 1 | sed 's/.*"\(.*\)"/\1/')

echo "Using certificate: $CERT_NAME"
echo "SHA-1: $CERT_SHA"
echo ""

# Ask for password
echo "Enter password for certificate export (will be used to encrypt .p12):"
read -s PASSWORD
echo ""

if [ -z "$PASSWORD" ]; then
    echo "❌ Password cannot be empty"
    exit 1
fi

echo "Confirm password:"
read -s PASSWORD_CONFIRM
echo ""

if [ "$PASSWORD" != "$PASSWORD_CONFIRM" ]; then
    echo "❌ Passwords do not match"
    exit 1
fi

echo "Exporting certificate..."

# Export to .p12
TEMP_P12=$(mktemp /tmp/playa-cert.XXXXXX.p12)
TEMP_CERT_TXT=$(mktemp /tmp/playa-cert.XXXXXX.txt)
TEMP_PASSWD_TXT=$(mktemp /tmp/playa-passwd.XXXXXX.txt)

security export -k login.keychain -t identities -f pkcs12 \
    -P "$PASSWORD" \
    -o "$TEMP_P12" \
    "$CERT_SHA"

echo "✓ Certificate exported to temporary file"

# Convert to base64
BASE64_CERT=$(base64 -i "$TEMP_P12")

# Save to temporary files (outside git repo)
echo "$BASE64_CERT" > "$TEMP_CERT_TXT"
echo "$PASSWORD" > "$TEMP_PASSWD_TXT"

echo ""
echo "========================================="
echo "✓ Export complete!"
echo "========================================="
echo ""

# Check if gh CLI is available
if command -v gh >/dev/null 2>&1; then
    echo "GitHub CLI detected. Upload secrets automatically? (y/n)"
    read -r UPLOAD_AUTO
    echo ""

    if [ "$UPLOAD_AUTO" = "y" ] || [ "$UPLOAD_AUTO" = "Y" ]; then
        echo "Uploading secrets to GitHub..."
        cat "$TEMP_CERT_TXT" | gh secret set APPLE_CERTIFICATE
        cat "$TEMP_PASSWD_TXT" | gh secret set APPLE_CERTIFICATE_PASSWORD
        echo "✅ Secrets uploaded successfully!"
        echo ""
        echo "Verify with: gh secret list"
    else
        echo "Manual upload instructions:"
        echo ""
        echo "1. Copy certificate to clipboard:"
        echo "   cat $TEMP_CERT_TXT | pbcopy"
        echo ""
        echo "2. Set APPLE_CERTIFICATE secret:"
        echo "   gh secret set APPLE_CERTIFICATE"
        echo "   (or paste manually at: https://github.com/ssoj13/playa/settings/secrets/actions)"
        echo ""
        echo "3. Copy password to clipboard:"
        echo "   cat $TEMP_PASSWD_TXT | pbcopy"
        echo ""
        echo "4. Set APPLE_CERTIFICATE_PASSWORD secret:"
        echo "   gh secret set APPLE_CERTIFICATE_PASSWORD"
        echo ""
    fi
else
    echo "⚠️  GitHub CLI (gh) not found. Manual upload required."
    echo ""
    echo "Temporary files created:"
    echo "  $TEMP_CERT_TXT"
    echo "  $TEMP_PASSWD_TXT"
    echo ""
    echo "To upload secrets:"
    echo "  cat $TEMP_CERT_TXT | pbcopy"
    echo "  # Paste at: https://github.com/ssoj13/playa/settings/secrets/actions"
    echo ""
    echo "  cat $TEMP_PASSWD_TXT | pbcopy"
    echo "  # Paste APPLE_CERTIFICATE_PASSWORD"
    echo ""
fi

# Cleanup
rm -f "$TEMP_P12"

echo ""
echo "⚠️  Remember to delete temporary files after uploading:"
echo "  rm -f $TEMP_CERT_TXT $TEMP_PASSWD_TXT"
echo ""
echo "Done! Next CI build will use code signing."
echo ""
