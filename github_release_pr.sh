#!/usr/bin/env bash
# Create a Pull Request from dev to main for release
#
# Usage:
#   ./create_release_pr.sh [version]
#
# Examples:
#   ./create_release_pr.sh v0.2.0
#   ./create_release_pr.sh         (will prompt for version)

set -e

# Get version from argument or prompt
VERSION="$1"
if [ -z "$VERSION" ]; then
    read -p "Enter release version (e.g., v0.2.0): " VERSION
fi

# Remove 'v' prefix if present, then add it back for consistency
VERSION="${VERSION#v}"
VERSION="v${VERSION}"

# Get commit count between main and dev
echo ""
echo "Calculating changes between main and dev..."
COMMIT_COUNT=$(git rev-list --count origin/main..dev)

# Create PR title and body
TITLE="Release ${VERSION}"
BODY="Release ${VERSION} - ${COMMIT_COUNT} commits from dev branch"

echo ""
echo "Creating Pull Request:"
echo "  From: dev"
echo "  To:   main"
echo "  Title: ${TITLE}"
echo "  Commits: ${COMMIT_COUNT}"
echo ""

# Create the PR
if ! gh pr create --base main --head dev --title "${TITLE}" --body "${BODY}"; then
    echo ""
    echo "Error: Failed to create pull request"
    echo "Make sure you have:"
    echo "  - Pushed your dev branch to origin"
    echo "  - Authenticated with 'gh auth login'"
    exit 1
fi

echo ""
echo "✓ Pull Request created successfully!"
echo ""
echo "Next steps:"
echo "  1. Review the PR on GitHub"
echo "  2. Merge when ready: gh pr merge --merge"
echo "  3. Create tag on main: git tag ${VERSION} && git push origin ${VERSION}"
echo "  4. Release workflow will create GitHub Release automatically"
