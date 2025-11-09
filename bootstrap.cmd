@echo off
:: Bootstrap script for playa project
:: Checks dependencies, builds xtask, and runs commands
::
:: Usage:
::   bootstrap.cmd                    # Show xtask help
::   bootstrap.cmd tag-dev patch      # Run xtask command
::   bootstrap.cmd build --release    # Run xtask command
::   bootstrap.cmd test               # Run encoding integration test
::   bootstrap.cmd install            # Install playa from crates.io (checks FFmpeg dependencies)
::   bootstrap.cmd publish            # Publish crate to crates.io
::   bootstrap.cmd wipe               # Clean .\target from stale platform binaries (non-recursive)
::   bootstrap.cmd wipe -v            # Verbose output
::   bootstrap.cmd wipe --dry-run     # Show what would be removed
::   bootstrap.cmd wipe-wf            # Delete all GitHub Actions workflow runs for this repo

setlocal enabledelayedexpansion

:: Set FFmpeg/vcpkg environment variables for this script session
if exist "C:\vcpkg" (
    set "VCPKG_ROOT=C:\vcpkg"
    set "VCPKGRS_TRIPLET=x64-windows-static-md-release"
    set "PKG_CONFIG_PATH=C:\vcpkg\installed\x64-windows-static-md-release\lib\pkgconfig"
)

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

:: Verify vcpkg configuration
if defined VCPKG_ROOT (
    echo ✓ vcpkg configured: %VCPKG_ROOT%
    echo ✓ triplet: %VCPKGRS_TRIPLET%
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

:: Handle special commands
if "%~1"=="test" (
    :: Run encoding integration test
    echo Running encoding integration test...
    echo.
    cargo test --release test_encode_placeholder_frames -- --nocapture
    goto :end
)

if "%~1"=="publish" (
    :: Publish crate to crates.io
    echo Publishing crate to crates.io...
    echo.
    cargo publish
    goto :end
)

if "%~1"=="install" (
    :: Install playa from crates.io with FFmpeg dependencies
    echo Checking FFmpeg dependencies...
    echo.

    :: Check vcpkg
    if not exist "C:\vcpkg" (
        echo Error: vcpkg not found at C:\vcpkg
        echo.
        set /p "install_vcpkg=Install vcpkg? (y/N): "
        if /i "!install_vcpkg!"=="y" (
            echo Installing vcpkg...
            git clone https://github.com/microsoft/vcpkg.git C:\vcpkg
            C:\vcpkg\bootstrap-vcpkg.bat
            echo ✓ vcpkg installed
        ) else (
            echo Installation cancelled.
            exit /b 1
        )
    ) else (
        echo ✓ vcpkg found
    )

    :: Check FFmpeg
    if not exist "C:\vcpkg\installed\x64-windows-static-md-release\lib\pkgconfig\libavutil.pc" (
        echo.
        echo Error: FFmpeg not found
        echo.
        set /p "install_ffmpeg=Install FFmpeg via vcpkg? (y/N): "
        if /i "!install_ffmpeg!"=="y" (
            echo Installing FFmpeg with hardware acceleration support...
            C:\vcpkg\vcpkg install ffmpeg[core,avcodec,avdevice,avfilter,avformat,swresample,swscale,nvcodec]:x64-windows-static-md-release
            echo ✓ FFmpeg installed
        ) else (
            echo Installation cancelled.
            exit /b 1
        )
    ) else (
        echo ✓ FFmpeg found
    )

    :: Check pkg-config
    where pkg-config >nul 2>&1
    if errorlevel 1 (
        echo.
        echo Error: pkg-config not found
        echo.
        set /p "install_pkgconfig=Install pkg-config via vcpkg? (y/N): "
        if /i "!install_pkgconfig!"=="y" (
            echo Installing pkg-config...
            C:\vcpkg\vcpkg install pkgconf:x64-windows-static-md-release
            echo ✓ pkg-config installed
        ) else (
            echo Installation cancelled.
            exit /b 1
        )
    ) else (
        echo ✓ pkg-config found
    )

    echo.
    echo Installing playa from crates.io...
    echo.
    cargo install playa
    goto :end
)

:: Run xtask with all arguments
if "%~1"=="" (
    :: No arguments - show help
    cargo xtask --help
) else (
    :: Pass all arguments to xtask
    cargo xtask %*
)

:end

endlocal
