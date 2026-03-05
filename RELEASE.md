# Release Process

This document describes the release process for Portal.

## Automated Release (Recommended)

Use the release script to automate version bumping and tag creation:

```bash
./scripts/release.sh 0.10.2
```

This script will:
1. Update `Cargo.toml` version to `0.10.2`
2. Update `Cargo.lock`
3. Commit the version changes
4. Create git tag `v0.10.2`
5. Push commit and tag to remote

## Manual Release

If you prefer manual release:

1. **Update version in Cargo.toml:**
   ```toml
   [package]
   version = "0.10.2"
   ```

2. **Update Cargo.lock:**
   ```bash
   cargo update
   ```

3. **Commit the changes:**
   ```bash
   git add Cargo.toml Cargo.lock
   git commit -m "Bump version to 0.10.2"
   git push
   ```

4. **Create and push git tag:**
   ```bash
   git tag -a v0.10.2 -m "Release v0.10.2"
   git push origin v0.10.2
   ```

## Building DMG Installer

After releasing a new version:

```bash
./scripts/build-dmg.sh
```

The script will automatically use the git tag version and generate:
- `target/release/Portal-<version>-<arch>.dmg`

Example: `target/release/Portal-0.10.2-arm64.dmg`

## Version Numbering

Follow [Semantic Versioning 2.0.0](https://semver.org/):
- **MAJOR** version: Incompatible API changes
- **MINOR** version: Backwards-compatible functionality additions
- **PATCH** version: Backwards-compatible bug fixes

Examples:
- `0.10.0` → `0.10.1` - Bug fixes
- `0.10.0` → `0.11.0` - New features
- `0.10.0` → `1.0.0` - Major breaking changes
