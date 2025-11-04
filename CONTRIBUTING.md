# Contributing to Playa

## Commit Message Convention

This project follows [Conventional Commits](https://www.conventionalcommits.org/) specification.

### Format

```
<type>(<scope>): <description>

[optional body]

[optional footer(s)]
```

### Types

- `feat:` - New feature
- `fix:` - Bug fix
- `docs:` - Documentation changes
- `chore:` - Routine tasks (dependency updates, releases)
- `refactor:` - Code refactoring without functionality changes
- `perf:` - Performance improvements
- `test:` - Adding or updating tests
- `ci:` - CI/CD configuration changes

### Examples

```bash
feat: Add HDR tone mapping support
fix: Resolve memory leak in image cache
docs: Update build instructions for Windows
chore: Bump image crate to 0.25
perf: Optimize EXR decoding with parallel loading
```

### Scope (Optional)

```bash
feat(ui): Add playback speed control
fix(cache): Prevent duplicate image loading
docs(readme): Add macOS build instructions
```

## Quick Commit Helper (Optional)

For rapid commits during development, you can use the `gpush2.cmd` helper script:

```bash
# Commit with custom message (auto-adds 'chore:' if no type specified)
gpush2.cmd "Add placeholder icon"
# → commits as "chore: Add placeholder icon"

# Commit with explicit type
gpush2.cmd "feat: Add playback speed indicator"
# → commits as "feat: Add playback speed indicator"

# Commit with scope
gpush2.cmd "fix(ui): Correct timeline alignment"
# → commits as "fix(ui): Correct timeline alignment"

# Interactive mode (prompts for message)
gpush2.cmd
# → Enter commit message: [your message]

# Quick WIP commit (empty input)
gpush2.cmd
# → [press Enter]
# → commits as "chore: WIP <timestamp>"
```

The script automatically:
- Stages all changes (`git add -A`)
- Commits with the message
- Pushes to origin with `--set-upstream`

**Note**: For proper conventional commits, always specify the type explicitly or let it default to `chore:`.

## Changelog Generation

The project uses [git-cliff](https://git-cliff.org/) to automatically generate `CHANGELOG.md` from commit messages.

### How It Works

CHANGELOG.md is generated automatically by **git-cliff** during the release process:

1. When you run `cargo xtask release`, the **cargo-release** tool starts the release process
2. Before creating the release commit, it executes the `pre-release-hook` defined in `Cargo.toml`:
   ```toml
   pre-release-hook = ["git-cliff", "--tag", "v{{version}}", "-o", "CHANGELOG.md"]
   ```
3. **git-cliff** reads all git commits since the last release tag
4. It parses commits following the [Conventional Commits](https://www.conventionalcommits.org/) format
5. It applies filtering and grouping rules from `cliff.toml`:
   - Filters out: release commits (`chore: Release`), checkpoints, and some dependency updates
   - Groups commits by type: Features, Bug Fixes, Documentation, etc.
   - Adds emoji icons for each category
   - Links issue numbers automatically (when available)
6. **git-cliff regenerates CHANGELOG.md from scratch** (not incremental) - it always rebuilds the entire changelog from all git history
7. The updated CHANGELOG.md is included in the release commit: `"chore: Release playa v0.1.x"`
8. The commit is tagged and pushed to GitHub

### Manual Commands

```bash
# Regenerate full CHANGELOG.md from all git history
./changelog.sh      # Linux/macOS
changelog.cmd       # Windows

# Preview unreleased changes (doesn't modify CHANGELOG.md)
cargo xtask changelog-preview
# Creates CHANGELOG.preview.md (git-ignored)
```

### Configuration Files

- **`Cargo.toml`** (line 71): Defines when git-cliff runs via `pre-release-hook`
- **`cliff.toml`**: Configures commit parsing, filtering, grouping, and output format
- **`changelog.sh`** / **`changelog.cmd`**: Manual scripts for regenerating CHANGELOG outside of release process

## Release Process

The release process uses a two-stage workflow:
1. **Build and tag** from `dev` branch
2. **Publish release** after merging to `main`

### Prerequisites

```bash
cargo install cargo-release
cargo install git-cliff  # Already installed
```

### Step 1: Build and Tag from Dev Branch

From the `dev` branch, run:

```bash
# Using convenience scripts:
./build_dev.sh patch        # Linux/macOS - patch version (0.1.23 -> 0.1.24)
build_dev.cmd patch         # Windows - patch version (0.1.23 -> 0.1.24)

# Or use cargo xtask directly:
cargo xtask release patch   # Patch version
cargo xtask release minor   # Minor version (0.1.23 -> 0.2.0)
cargo xtask release major   # Major version (0.1.23 -> 1.0.0)

# Dry run (test without committing)
cargo xtask release patch --dry-run
```

### What Happens in Step 1

1. **git-cliff** updates `CHANGELOG.md` with all commits since last release
2. **cargo-release** bumps version in `Cargo.toml`
3. Creates commit: `"chore: Release playa v0.1.24"`
4. Creates git tag: `v0.1.24`
5. Pushes `dev` branch and tag to GitHub
6. **Build workflow** runs and creates test artifacts (retained 7 days)

### Step 2: Test and Merge to Main

1. **Download artifacts** from GitHub Actions (https://github.com/ssoj13/playa/actions)
2. **Test the build** on your target platforms
3. **Merge to main**:
   - Option A: Create PR from `dev` to `main` on GitHub
   - Option B: Manually merge and push to `main`

### Step 3: Release Workflow (Automatic)

When you merge the tag to `main`:
- **Release workflow** automatically triggers
- Builds all artifacts again
- **Creates GitHub Release** with installers attached

### Release Configuration

Located in `Cargo.toml`:

```toml
[package.metadata.release]
pre-release-commit-message = "chore: Release {{crate_name}} v{{version}}"
pre-release-hook = ["git-cliff", "--tag", "v{{version}}", "-o", "CHANGELOG.md"]
publish = false  # Don't publish to crates.io
```

## CI/CD Pipeline

### Workflows

The project uses two separate GitHub Actions workflows:

#### 1. **Build Workflow** (`build.yml`) - Development & Testing
- **Triggers**: Push to `dev` branch, PRs to `main`/`dev`
- **Purpose**: Build and test artifacts without publishing
- **Artifacts**: Windows installer, portable ZIP, Linux AppImage, DEB package
- **Retention**: 7 days
- **Use case**: Test builds before merging to main

#### 2. **Release Workflow** (`release.yml`) - Production Releases
- **Triggers**: Tags matching `v*` pattern **from main branch only**
- **Purpose**: Build, package, and publish official releases
- **Artifacts**: Same as build workflow + GitHub Release creation
- **Use case**: Official versioned releases

### Build Performance

- **Cold cache** (first build): ~35 minutes
- **Warm cache** (subsequent): ~2-3 minutes (93% faster!)
- **cargo-binstall** (cargo-packager): ~5-10 seconds

### Caching Strategy

#### 1. Binary Tools (`cargo-binstall`)
- Downloads precompiled `cargo-packager` binary instead of compiling
- **Why**: GitHub Cache API is unstable - cache randomly fails even when it exists
- **Speed**: ~5-10 seconds vs ~3-5 minutes compilation
- No caching needed - downloads are fast enough

#### 2. Rust Dependencies Cache (`rust-cache`)
- Caches compiled Rust dependencies
- Configuration: `cache-bin: false` to avoid binary conflicts

#### 3. C++ Compilation Cache (`sccache` - Linux only)
- Caches C++ object files from `openexr-sys` build (~30 min → ~2 min)
- **Platform**: Linux only - disabled on Windows due to GitHub Cache API instability
- **Requires**: `CARGO_INCREMENTAL=0` (auto-set by `dtolnay/rust-toolchain`)
- **Why**: Incremental compilation conflicts with sccache
- **Fallback**: Automatic connectivity test; disables if GitHub Cache API is down

### Why CARGO_INCREMENTAL=0?

sccache works at the rustc compilation unit level, while incremental compilation caches at a different granularity. Having both enabled causes cache conflicts and negates sccache benefits.

### Caching Order Matters

**Windows:**
```yaml
1. Install Rust toolchain
2. Install cargo-binstall
3. Install cargo-packager (via binstall)
4. Cache rust dependencies (with cache-bin: false)
5. Build application
```

**Linux:**
```yaml
1. Install Rust toolchain (sets CARGO_INCREMENTAL=0)
2. Install sccache (with fallback)
3. Test sccache connectivity (disable if fails)
4. Install cargo-binstall
5. Install cargo-packager (via binstall)
6. Cache rust dependencies (with cache-bin: false)
7. Build application
```

**Why no sccache on Windows?** GitHub Cache API returns intermittent 400 errors on Windows runners, causing build failures even with fallback logic. Linux remains stable.

### Manual Workflow Trigger

You can manually trigger builds from GitHub Actions UI with custom parameters.

## Build Instructions

See `README.md` for platform-specific build instructions (Windows/Linux/macOS).

## Development Workflow

1. Make changes
2. Commit with conventional commit format
3. Push to branch
4. Create PR
5. After merge to main, create release when ready
6. GitHub Actions handles the rest

## Issue and Pull Request Process

### Creating an Issue

When creating an issue, please use the appropriate template:
- **Bug Report**: For reproducible bugs with steps and environment details
- **Feature Request**: For new functionality suggestions

Fill out all required fields to help us understand and address your request quickly.

### Creating a Pull Request

1. Fork the repository and create a feature branch
2. Make your changes following the commit conventions
3. Ensure all tests pass and code is formatted:
   ```bash
   cargo fmt
   cargo clippy -- -D warnings
   cargo test
   ```
4. Update CHANGELOG.md if applicable (or let git-cliff do it on release)
5. Create PR using the template - fill out all sections
6. Wait for CI checks to pass
7. Address review feedback

### Dependabot PRs

Dependabot automatically creates PRs for dependency updates weekly:
- **Minor/Patch updates**: Usually safe to merge after CI passes
- **Major updates**: Review breaking changes before merging
- **Grouped updates**: Multiple related dependencies updated together

Review the PR description for changelog and breaking change details.

## Questions?

- Check existing issues: https://github.com/ssoj13/playa/issues
- CI/CD logs: https://github.com/ssoj13/playa/actions
