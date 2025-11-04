@echo off
:: Build and tag release from dev branch
::
:: This script is a wrapper around 'cargo xtask release' for convenience.
:: You can also call 'cargo xtask release' directly.
::
:: Usage:
::   github_deploy_dev.cmd [patch|minor|major] [--dry-run]
::
:: Examples:
::   github_deploy_dev.cmd patch          - Create patch release (0.1.13 -> 0.1.14)
::   github_deploy_dev.cmd minor          - Create minor release (0.1.13 -> 0.2.0)
::   github_deploy_dev.cmd major          - Create major release (0.1.13 -> 1.0.0)
::   github_deploy_dev.cmd patch --dry-run - Test without making changes
::
:: What happens:
::   1. Updates version in Cargo.toml
::   2. Generates CHANGELOG.md
::   3. Creates commit and tag
::   4. Pushes current branch (dev) and tags to GitHub
::   5. Build workflow runs and creates artifacts for testing
::   6. After testing, merge to main manually or via PR
::   7. Release workflow publishes the official release

setlocal

:: Check if xtask is available
cargo xtask --help >nul 2>&1
if errorlevel 1 (
    echo Error: cargo xtask is not available.
    echo The xtask crate may not be built yet.
    echo.
    echo Solution: Run 'cargo build -p xtask' first to build the xtask tool.
    exit /b 1
)

:: Check if cargo-release is installed
cargo release --version >nul 2>&1
if errorlevel 1 (
    echo Error: cargo-release is not installed.
    echo.
    echo Solution: Install it with: cargo install cargo-release
    exit /b 1
)

:: Get release level from argument (patch, minor, major), default to patch
set LEVEL=%1
if "%LEVEL%"=="" set LEVEL=patch

:: Get dry-run flag from argument
set DRY_RUN=%2

:: Build xtask command
set CMD=cargo xtask release %LEVEL%

if "%DRY_RUN%"=="--dry-run" (
    set CMD=%CMD% --dry-run
)

:: Run the command
echo Running: %CMD%
echo.
%CMD%

endlocal
