@echo off
:: Release script - now uses cargo xtask
::
:: This script is a wrapper around 'cargo xtask release' for convenience.
:: You can also call 'cargo xtask release' directly.
::
:: Usage:
::   release.cmd [patch|minor|major] [--dry-run]
::
:: Examples:
::   release.cmd patch          - Create patch release (0.1.13 -> 0.1.14)
::   release.cmd minor          - Create minor release (0.1.13 -> 0.2.0)
::   release.cmd major          - Create major release (0.1.13 -> 1.0.0)
::   release.cmd patch --dry-run - Test without making changes

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