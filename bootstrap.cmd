@echo off
:: Bootstrap script for playa project
:: Checks dependencies, builds xtask, and runs commands
::
:: Usage:
::   bootstrap.cmd                    # Show xtask help
::   bootstrap.cmd tag-dev patch      # Run xtask command
::   bootstrap.cmd build --release    # Run xtask command

setlocal enabledelayedexpansion

:: Check if cargo is installed
where cargo >nul 2>&1
if errorlevel 1 (
    echo Error: Rust/Cargo not found!
    echo.
    echo Please install Rust from: https://rustup.rs/
    exit /b 1
)

echo Checking dependencies...
echo.

:: Check if cargo-release is installed
cargo release --version >nul 2>&1
if errorlevel 1 (
    echo [1/2] Installing cargo-release...
    cargo install cargo-release
    if errorlevel 1 (
        echo Error: Failed to install cargo-release
        exit /b 1
    )
    echo   ✓ cargo-release installed
) else (
    echo [1/2] ✓ cargo-release already installed
)

:: Check if cargo-packager is installed
cargo packager --version >nul 2>&1
if errorlevel 1 (
    echo [2/2] Installing cargo-packager...
    cargo install cargo-packager --version 0.11.7 --locked
    if errorlevel 1 (
        echo Error: Failed to install cargo-packager
        exit /b 1
    )
    echo   ✓ cargo-packager installed
) else (
    echo [2/2] ✓ cargo-packager already installed
)

echo.
echo Dependencies ready!
echo.

:: Check if xtask is built
if not exist "target\debug\xtask.exe" (
    echo Building xtask...
    cargo build -p xtask
    if errorlevel 1 (
        echo Error: Failed to build xtask
        exit /b 1
    )
    echo ✓ xtask built
    echo.
)

:: Run xtask with all arguments
if "%~1"=="" (
    :: No arguments - show help
    cargo xtask --help
) else (
    :: Pass all arguments to xtask
    cargo xtask %*
)

endlocal
