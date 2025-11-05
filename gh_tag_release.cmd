@echo off
:: Create release tag on main branch for official releases
::
:: Usage:
::   gh_tag_release.cmd [patch|minor|major] [--dry-run]
::
:: Examples:
::   gh_tag_release.cmd patch      - Create v0.1.14 tag on main
::   gh_tag_release.cmd minor      - Create v0.2.0 tag on main
::   gh_tag_release.cmd --dry-run  - Test without making changes
::
:: IMPORTANT: Run this ONLY on main branch after merging from dev!
::
:: What happens:
::   1. Updates version in Cargo.toml
::   2. Generates CHANGELOG.md
::   3. Creates commit and tag (NO -dev suffix)
::   4. Pushes to main branch
::   5. Release workflow creates official GitHub Release

setlocal

:: Check if on main branch
for /f "tokens=*" %%i in ('git branch --show-current') do set CURRENT_BRANCH=%%i

if not "%CURRENT_BRANCH%"=="main" (
    echo Error: You must be on main branch to create a release tag!
    echo Current branch: %CURRENT_BRANCH%
    echo.
    echo Solution:
    echo   1. git checkout main
    echo   2. git merge dev
    echo   3. Run this script again
    exit /b 1
)

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

:: Build xtask command (NO metadata = no -dev suffix)
set CMD=cargo xtask release %LEVEL%

if "%DRY_RUN%"=="--dry-run" (
    set CMD=%CMD% --dry-run
)

:: Run the command
echo Running: %CMD%
echo.
echo This will create an official release tag (e.g., v0.1.14)
echo Release workflow will create GitHub Release with installers
echo.
%CMD%

endlocal
