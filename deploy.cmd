@echo off
:: Deploy script using cargo xtask
:: Builds and packages the application with all dependencies

:: Check if xtask is available
cargo xtask --help >nul 2>&1
if errorlevel 1 (
    echo Error: cargo xtask is not available.
    echo The xtask crate may not be built yet.
    echo.
    echo Solution: Run 'cargo build -p xtask' first to build the xtask tool.
    exit /b 1
)

:: Check if cargo packager is available
cargo packager --version >nul 2>&1
if errorlevel 1 (
    echo Error: cargo-packager is not installed.
    echo.
    echo Solution: Install it with: cargo install cargo-packager
    exit /b 1
)

cargo xtask build --release && cargo packager --release
