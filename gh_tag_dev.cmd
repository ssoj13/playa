@echo off
:: Create dev tag with -dev suffix for testing
::
:: Usage:
::   gh_tag_dev.cmd [patch|minor|major] [--dry-run]
::
:: Examples:
::   gh_tag_dev.cmd patch          - Create v0.1.14-dev tag
::   gh_tag_dev.cmd minor          - Create v0.2.0-dev tag
::   gh_tag_dev.cmd --dry-run      - Test without making changes
::
:: What happens:
::   1. Updates version in Cargo.toml
::   2. Generates CHANGELOG.md
::   3. Creates commit and tag with -dev suffix
::   4. Pushes to dev branch
::   5. Build workflow creates test artifacts (NOT release)

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

:: Build xtask command with -dev suffix
set CMD=cargo xtask release %LEVEL% --metadata dev

if "%DRY_RUN%"=="--dry-run" (
    set CMD=%CMD% --dry-run
)

:: Run the command
echo Running: %CMD%
echo.
echo This will create a tag with -dev suffix (e.g., v0.1.14-dev)
echo Build workflow will create test artifacts (NOT GitHub Release)
echo.
%CMD%

endlocal
