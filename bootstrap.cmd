@echo off
:: Bootstrap script for playa project
:: Checks dependencies, builds xtask, and runs commands
::
:: Usage:
::   bootstrap.cmd                    # Show xtask help
::   bootstrap.cmd tag-dev patch      # Run xtask command
::   bootstrap.cmd build --release    # Run xtask command
::   bootstrap.cmd wipe               # Clean .\target from stale platform binaries (non-recursive)
::   bootstrap.cmd wipe -v            # Verbose output
::   bootstrap.cmd wipe --dry-run     # Show what would be removed
::   bootstrap.cmd wipe-wf            # Delete all GitHub Actions workflow runs for this repo

setlocal enabledelayedexpansion

:: Check if cargo is installed
where cargo >nul 2>&1
if errorlevel 1 (
    echo Error: Rust/Cargo not found!
    echo.
    echo Please install Rust from: https://rustup.rs/
    exit /b 1
)

:: Setup Visual Studio environment
echo Setting up build environment...
set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
if exist "%VSWHERE%" (
    for /f "usebackq tokens=*" %%i in (`"%VSWHERE%" -latest -property installationPath`) do (
        set "VCVARS=%%i\VC\Auxiliary\Build\vcvars64.bat"
        if exist "!VCVARS!" (
            call "!VCVARS!" >nul 2>&1
            echo ✓ Visual Studio environment configured
        )
    )
)

:: Setup vcpkg
if exist "C:\vcpkg" (
    set "VCPKG_ROOT=C:\vcpkg"
    set "PKG_CONFIG_PATH=C:\vcpkg\installed\x64-windows-static-md\lib\pkgconfig"
    echo ✓ vcpkg configured
)
echo.

echo Checking dependencies...
echo.

:: Check if cargo-binstall is installed
cargo binstall --version >nul 2>&1
if errorlevel 1 (
    echo [1/3] Installing cargo-binstall...
    cargo install cargo-binstall
    if errorlevel 1 (
        echo Error: Failed to install cargo-binstall
        exit /b 1
    )
    echo   ✓ cargo-binstall installed
) else (
    echo [1/3] ✓ cargo-binstall already installed
)

:: Check if cargo-release is installed
cargo release --version >nul 2>&1
if errorlevel 1 (
    echo [2/3] Installing cargo-release...
    cargo binstall cargo-release --no-confirm
    if errorlevel 1 (
        echo   Falling back to cargo install...
        cargo install cargo-release
        if errorlevel 1 (
            echo Error: Failed to install cargo-release
            exit /b 1
        )
    )
    echo   ✓ cargo-release installed
) else (
    echo [2/3] ✓ cargo-release already installed
)

:: Check if cargo-packager is installed
cargo packager --version >nul 2>&1
if errorlevel 1 (
    echo [3/3] Installing cargo-packager...
    cargo binstall cargo-packager --version 0.11.7 --no-confirm
    if errorlevel 1 (
        echo   Falling back to cargo install...
        cargo install cargo-packager --version 0.11.7 --locked
        if errorlevel 1 (
            echo Error: Failed to install cargo-packager
            exit /b 1
        )
    )
    echo   ✓ cargo-packager installed
) else (
    echo [3/3] ✓ cargo-packager already installed
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
