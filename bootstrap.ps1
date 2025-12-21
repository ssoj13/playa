#!/usr/bin/env pwsh
# Bootstrap script for playa project
# Checks dependencies, builds xtask, and runs commands
#
# Usage:
#   .\bootstrap.ps1                    # Show bootstrap help
#   .\bootstrap.ps1 tag-dev patch      # Run xtask command
#   .\bootstrap.ps1 build --release    # Run xtask command
#   .\bootstrap.ps1 test               # Run all tests via xtask
#   .\bootstrap.ps1 install            # Install playa from crates.io (checks FFmpeg dependencies)
#   .\bootstrap.ps1 publish            # Publish crate to crates.io

# Set default vcpkg paths if not already set
if (-not $env:VCPKG_ROOT) {
    if (Test-Path 'C:\vcpkg') {
        $env:VCPKG_ROOT = 'C:\vcpkg'
    }
}

if ($env:VCPKG_ROOT -and -not $env:VCPKGRS_TRIPLET) {
    $env:VCPKGRS_TRIPLET = 'x64-windows-static-md-release'
}

if ($env:VCPKG_ROOT -and $env:VCPKGRS_TRIPLET) {
    $env:PKG_CONFIG_PATH = Join-Path $env:VCPKG_ROOT "installed\$env:VCPKGRS_TRIPLET\lib\pkgconfig"
}

# Check if cargo is installed
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Host 'Error: Rust/Cargo not found!' -ForegroundColor Red
    Write-Host ''
    Write-Host 'Please install Rust from: https://rustup.rs/'
    exit 1
}

# Setup Visual Studio environment using vcv-rs (fast) or vcvars64.bat (fallback)
Write-Host 'Setting up build environment...'

# Try vcv-rs first (~50x faster than vcvars64.bat)
$vcvFound = $false
if (Get-Command vcv-rs -ErrorAction SilentlyContinue) {
    try {
        vcv-rs -q -f ps | Invoke-Expression
        $vcvFound = $true
        Write-Host '[OK] Visual Studio environment configured (vcv-rs)' -ForegroundColor Green
    } catch {
        Write-Host '  vcv-rs failed, falling back to vcvars64.bat...' -ForegroundColor Yellow
    }
} else {
    # vcv-rs not found - try to install it
    Write-Host '  vcv-rs not found, installing...'
    cargo install vcv-rs --quiet 2>$null
    if ($LASTEXITCODE -eq 0 -and (Get-Command vcv-rs -ErrorAction SilentlyContinue)) {
        try {
            vcv-rs -q -f ps | Invoke-Expression
            $vcvFound = $true
            Write-Host '[OK] Visual Studio environment configured (vcv-rs)' -ForegroundColor Green
        } catch {
            Write-Host '  vcv-rs failed after install, falling back...' -ForegroundColor Yellow
        }
    }
}

# Fallback to vcvars64.bat if vcv-rs didn't work
if (-not $vcvFound) {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (Test-Path $vswhere) {
        $installPath = & $vswhere -latest -property installationPath
        if ($installPath) {
            $vcvars = Join-Path $installPath 'VC\Auxiliary\Build\vcvars64.bat'
            if (Test-Path $vcvars) {
                # Import VS environment variables
                $output = cmd /c "`"$vcvars`" && set" 2>&1
                $output | ForEach-Object {
                    if ($_ -match '^([^=]+)=(.*)$') {
                        Set-Item -Path "env:$($matches[1])" -Value $matches[2]
                    }
                }
                Write-Host '[OK] Visual Studio environment configured (vcvars64.bat)' -ForegroundColor Green
            }
        }
    }
}

# Verify vcpkg configuration
if ($env:VCPKG_ROOT) {
    Write-Host "[OK] vcpkg configured: $env:VCPKG_ROOT" -ForegroundColor Green
    Write-Host "[OK] triplet: $env:VCPKGRS_TRIPLET" -ForegroundColor Green
}

# Clear LIBCLANG_PATH if it points to non-MSVC clang (e.g. ESP32 Xtensa)
# bindgen will use its bundled libclang which works with MSVC headers
if ($env:LIBCLANG_PATH -and $env:LIBCLANG_PATH -match 'esp|xtensa') {
    Write-Host "[WARN] Clearing LIBCLANG_PATH (ESP32 clang incompatible with MSVC)" -ForegroundColor Yellow
    Remove-Item Env:LIBCLANG_PATH -ErrorAction SilentlyContinue
}

Write-Host ''

Write-Host 'Checking dependencies...'
Write-Host ''

# Check if cargo-binstall is installed
if (-not (cargo binstall --version 2>$null)) {
    Write-Host '[1/3] Installing cargo-binstall...'
    cargo install cargo-binstall
    if ($LASTEXITCODE -ne 0) {
        Write-Host 'Error: Failed to install cargo-binstall' -ForegroundColor Red
        exit 1
    }
    Write-Host '  [OK] cargo-binstall installed' -ForegroundColor Green
} else {
    Write-Host '[1/3] [OK] cargo-binstall already installed' -ForegroundColor Green
}

# Check if cargo-release is installed
if (-not (cargo release --version 2>$null)) {
    Write-Host '[2/3] Installing cargo-release...'
    cargo binstall cargo-release --no-confirm
    if ($LASTEXITCODE -ne 0) {
        Write-Host '  Falling back to cargo install...'
        cargo install cargo-release
        if ($LASTEXITCODE -ne 0) {
            Write-Host 'Error: Failed to install cargo-release' -ForegroundColor Red
            exit 1
        }
    }
    Write-Host '  [OK] cargo-release installed' -ForegroundColor Green
} else {
    Write-Host '[2/3] [OK] cargo-release already installed' -ForegroundColor Green
}

# Check if cargo-packager is installed
if (-not (cargo packager --version 2>$null)) {
    Write-Host '[3/3] Installing cargo-packager...'
    cargo binstall cargo-packager --version 0.11.7 --no-confirm
    if ($LASTEXITCODE -ne 0) {
        Write-Host '  Falling back to cargo install...'
        cargo install cargo-packager --version 0.11.7 --locked
        if ($LASTEXITCODE -ne 0) {
            Write-Host 'Error: Failed to install cargo-packager' -ForegroundColor Red
            exit 1
        }
    }
    Write-Host '  [OK] cargo-packager installed' -ForegroundColor Green
} else {
    Write-Host '[3/3] [OK] cargo-packager already installed' -ForegroundColor Green
}

Write-Host ''
Write-Host 'Dependencies ready!' -ForegroundColor Green
Write-Host ''

# Check if xtask is built
if (-not (Test-Path 'target\debug\xtask.exe')) {
    Write-Host 'Building xtask...'
    cargo build -p xtask
    if ($LASTEXITCODE -ne 0) {
        Write-Host 'Error: Failed to build xtask' -ForegroundColor Red
        exit 1
    }
    Write-Host '[OK] xtask built' -ForegroundColor Green
    Write-Host ''
}

# Handle special commands
if ($args[0] -eq 'publish') {
    Write-Host 'Publishing crate to crates.io...'
    Write-Host ''
    cargo publish
    exit $LASTEXITCODE
}

if ($args[0] -eq 'install') {
    Write-Host 'Checking FFmpeg dependencies...'
    Write-Host ''

    # Determine vcpkg root
    $vcpkgRoot = $env:VCPKG_ROOT
    if (-not $vcpkgRoot) {
        $vcpkgRoot = 'C:\vcpkg'
    }

    # Check vcpkg
    if (-not (Test-Path $vcpkgRoot)) {
        Write-Host "Error: vcpkg not found at $vcpkgRoot" -ForegroundColor Red
        Write-Host ''
        $installVcpkg = Read-Host 'Install vcpkg? (y/N)'
        if ($installVcpkg -eq 'y' -or $installVcpkg -eq 'Y') {
            Write-Host 'Installing vcpkg...'
            git clone https://github.com/microsoft/vcpkg.git $vcpkgRoot
            & "$vcpkgRoot\bootstrap-vcpkg.bat"
            Write-Host '[OK] vcpkg installed' -ForegroundColor Green
        } else {
            Write-Host 'Installation cancelled.'
            exit 1
        }
    } else {
        Write-Host '[OK] vcpkg found' -ForegroundColor Green
    }

    # Check FFmpeg
    $triplet = $env:VCPKGRS_TRIPLET
    if (-not $triplet) {
        $triplet = 'x64-windows-static-md-release'
    }
    $ffmpegPath = Join-Path $vcpkgRoot "installed\$triplet\lib\pkgconfig\libavutil.pc"
    if (-not (Test-Path $ffmpegPath)) {
        Write-Host ''
        Write-Host 'Error: FFmpeg not found' -ForegroundColor Red
        Write-Host ''
        $installFfmpeg = Read-Host 'Install FFmpeg via vcpkg? (y/N)'
        if ($installFfmpeg -eq 'y' -or $installFfmpeg -eq 'Y') {
            Write-Host 'Installing FFmpeg with hardware acceleration support...'
            $vcpkgExe = Join-Path $vcpkgRoot 'vcpkg.exe'
            & $vcpkgExe install "ffmpeg[core,avcodec,avdevice,avfilter,avformat,swresample,swscale,nvcodec]:$triplet"
            Write-Host '[OK] FFmpeg installed' -ForegroundColor Green
        } else {
            Write-Host 'Installation cancelled.'
            exit 1
        }
    } else {
        Write-Host '[OK] FFmpeg found' -ForegroundColor Green
    }

    # Check pkg-config
    if (-not (Get-Command pkg-config -ErrorAction SilentlyContinue)) {
        Write-Host ''
        Write-Host 'Error: pkg-config not found' -ForegroundColor Red
        Write-Host ''
        $installPkgConfig = Read-Host 'Install pkg-config via vcpkg? (y/N)'
        if ($installPkgConfig -eq 'y' -or $installPkgConfig -eq 'Y') {
            Write-Host 'Installing pkg-config...'
            $vcpkgExe = Join-Path $vcpkgRoot 'vcpkg.exe'
            & $vcpkgExe install "pkgconf:$triplet"
            Write-Host '[OK] pkg-config installed' -ForegroundColor Green
        } else {
            Write-Host 'Installation cancelled.'
            exit 1
        }
    } else {
        Write-Host '[OK] pkg-config found' -ForegroundColor Green
    }

    Write-Host ''
    Write-Host 'Installing playa from crates.io...'
    Write-Host ''
    cargo install playa
    exit $LASTEXITCODE
}

# Run xtask with all arguments
if ($args.Count -eq 0) {
    # No arguments - show bootstrap help
    Write-Host 'Bootstrap script for playa project'
    Write-Host ''
    Write-Host 'USAGE:'
    Write-Host '  .\bootstrap.ps1 [COMMAND] [OPTIONS]'
    Write-Host ''
    Write-Host 'SPECIAL COMMANDS:'
    Write-Host '  install            Install playa from crates.io (checks FFmpeg deps)'
    Write-Host '  publish            Publish crate to crates.io'
    Write-Host ''
    Write-Host 'XTASK COMMANDS (forwarded to cargo xtask):'
    Write-Host '  build              Build playa (use --openexr for full EXR support)'
    Write-Host '  test               Run all tests (unit + integration)'
    Write-Host '  post               Copy native libraries (OpenEXR builds only)'
    Write-Host '  verify             Verify dependencies present'
    Write-Host '  deploy             Install to system'
    Write-Host '  tag-dev            Create dev tag (triggers Build workflow)'
    Write-Host '  tag-rel            Create release tag (triggers Release workflow)'
    Write-Host '  pr                 Create PR: dev -> main'
    Write-Host '  changelog          Preview unreleased CHANGELOG.md'
    Write-Host '  wipe               Clean target directory from stale binaries'
    Write-Host '  wipe-wf            Delete all GitHub workflow runs'
    Write-Host '  pre                Linux only: Patch OpenEXR headers'
    Write-Host ''
    Write-Host 'EXAMPLES:'
    Write-Host '  .\bootstrap.ps1                    # Show this help'
    Write-Host '  .\bootstrap.ps1 build --release    # Build release binary'
    Write-Host '  .\bootstrap.ps1 test               # Run all tests'
    Write-Host '  .\bootstrap.ps1 tag-dev patch      # Create v0.1.x-dev tag'
    Write-Host ''
    Write-Host 'For xtask command details, run: .\bootstrap.ps1 [command] --help'
} else {
    # Pass all arguments to xtask
    cargo xtask @args
    exit $LASTEXITCODE
}
