#!/usr/bin/env bash
set -euo pipefail

# Release script: update Cargo.toml version, commit, create and push git tag
#
# Usage:
#   ./scripts/release.sh 0.10.2

if [ $# -ne 1 ]; then
    echo "Usage: $0 <version>"
    echo "Example: $0 0.10.2"
    exit 1
fi

VERSION="$1"
# Validate version format (semver)
if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "Error: Invalid version format '$VERSION'"
    echo "Expected format: X.Y.Z (e.g., 0.10.2)"
    exit 1
fi

# Check if tag already exists
if git rev-parse "v$VERSION" >/dev/null 2>&1; then
    echo "Error: Tag v$VERSION already exists"
    exit 1
fi

echo "==> Releasing version $VERSION"

# Update Cargo.toml version
echo "==> Updating Cargo.toml..."
sed -i.bak "s/^version = \".*\"/version = \"$VERSION\"/" Cargo.toml
rm -f Cargo.toml.bak

# Update Cargo.lock (cargo will do this automatically)
echo "==> Updating Cargo.lock..."
cargo update

# Commit the version changes
echo "==> Committing version changes..."
git add Cargo.toml Cargo.lock
git commit -m "Bump version to $VERSION"

# Create and push tag
echo "==> Creating git tag v$VERSION..."
git tag -a "v$VERSION" -m "Release v$VERSION"

echo "==> Pushing commit and tag..."
git push
git push origin "v$VERSION"

echo ""
echo "==> Release v$VERSION complete!"
echo "   - Cargo.toml updated to $VERSION"
echo "   - Git tag v$VERSION created and pushed"
echo ""
echo "Next steps:"
echo "   1. Wait for CI/CD checks"
echo "   2. ./scripts/build-dmg.sh  # Build DMG installer"
