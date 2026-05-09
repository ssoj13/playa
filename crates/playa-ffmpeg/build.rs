use std::env;
use std::process::Command;

fn main() {
    // Try vcpkg on all platforms (not just Windows)
    // Only try vcpkg if FFMPEG_DIR is not explicitly set
    if env::var("FFMPEG_DIR").is_err() {
        // First, try to find existing FFmpeg installation via vcpkg
        match vcpkg::find_package("ffmpeg") {
            Ok(lib) => {
                println!("cargo:warning=Found FFmpeg via vcpkg");

                // Emit include paths for ffmpeg-sys-next
                for path in &lib.include_paths {
                    println!("cargo:include={}", path.display());
                }

                println!("cargo:rerun-if-env-changed=VCPKG_ROOT");
            }
            Err(_) => {
                // If not found, try to install it automatically
                if let Ok(vcpkg_root) = env::var("VCPKG_ROOT") {
                    println!("cargo:warning=FFmpeg not found in vcpkg, attempting automatic installation...");

                    let triplet = get_vcpkg_triplet();
                    let vcpkg_exe = if cfg!(target_os = "windows") { format!("{}/vcpkg.exe", vcpkg_root) } else { format!("{}/vcpkg", vcpkg_root) };

                    // Install FFmpeg via vcpkg
                    let status = Command::new(&vcpkg_exe).args(["install", &format!("ffmpeg:{}", triplet)]).status();

                    match status {
                        Ok(s) if s.success() => {
                            println!("cargo:warning=Successfully installed FFmpeg via vcpkg");
                            // Try to find it again after installation
                            if let Ok(lib) = vcpkg::find_package("ffmpeg") {
                                for path in &lib.include_paths {
                                    println!("cargo:include={}", path.display());
                                }
                            }
                        }
                        Ok(s) => {
                            println!("cargo:warning=vcpkg install failed with status: {}", s);
                            println!("cargo:warning=Falling back to system FFmpeg or pkg-config");
                        }
                        Err(e) => {
                            println!("cargo:warning=Failed to run vcpkg: {}", e);
                            println!("cargo:warning=Falling back to system FFmpeg or pkg-config");
                        }
                    }
                } else {
                    println!("cargo:warning=VCPKG_ROOT not set, falling back to system FFmpeg or pkg-config");
                }

                println!("cargo:rerun-if-env-changed=VCPKG_ROOT");
            }
        }
    }

    // Process FFmpeg feature flags from ffmpeg-sys-next
    for (name, value) in env::vars() {
        if name.starts_with("DEP_FFMPEG_") {
            if value == "true" {
                println!(r#"cargo:rustc-cfg=feature="{}""#, name["DEP_FFMPEG_".len()..name.len()].to_lowercase());
            }
            println!(r#"cargo:rustc-check-cfg=cfg(feature, values("{}"))"#, name["DEP_FFMPEG_".len()..name.len()].to_lowercase());
        }
    }

    // Link platform system libraries required by FFmpeg static build.
    // Mirrors the rustflags previously kept in .cargo/config.toml — moved here
    // so the linkage applies when this crate is built as a workspace member
    // (rustflags from per-crate config.toml are ignored in workspace context).
    #[cfg(target_os = "windows")]
    {
        // System libs FFmpeg pulls in via WinSDK / DirectShow / Media Foundation /
        // BCrypt / WinSock / GDI / VFW. `msvcprt` is the MSVC C++ runtime —
        // FFmpeg 8.1+ avfilter/vsrc_gfxcapture uses C++ <regex>, so we need it
        // alongside the default C runtime that rustc already links.
        for lib in [
            "bcrypt", "user32", "ole32", "oleaut32", "mfuuid", "strmiids",
            "mfplat", "secur32", "ws2_32", "shlwapi", "gdi32", "vfw32", "uuid",
            "msvcprt",
        ] {
            println!("cargo:rustc-link-lib={lib}");
        }
    }

    #[cfg(target_os = "macos")]
    {
        for fw in ["CoreFoundation", "CoreMedia", "CoreVideo", "VideoToolbox", "AudioToolbox", "Security"] {
            println!("cargo:rustc-link-lib=framework={fw}");
        }
        for lib in ["iconv", "bz2", "z"] {
            println!("cargo:rustc-link-lib={lib}");
        }
    }

    #[cfg(target_os = "linux")]
    {
        for lib in ["m", "pthread", "dl"] {
            println!("cargo:rustc-link-lib={lib}");
        }
    }
}

fn get_vcpkg_triplet() -> String {
    // Respect VCPKG_DEFAULT_TRIPLET if set (e.g., for release-only builds)
    if let Ok(triplet) = env::var("VCPKG_DEFAULT_TRIPLET") {
        return triplet;
    }

    // Otherwise use platform defaults
    if cfg!(target_os = "windows") {
        if cfg!(target_env = "msvc") {
            // Use static-md for static linking with dynamic CRT
            "x64-windows-static-md".to_string()
        } else {
            "x64-mingw-static".to_string()
        }
    } else if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") { "arm64-osx-release".to_string() } else { "x64-osx-release".to_string() }
    } else {
        // Linux - static linking
        "x64-linux-release".to_string()
    }
}
