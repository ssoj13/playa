8.0.0
-----

**FORK BY ALEX JOSS (playa-ffmpeg)**

This is a modernized fork with cross-platform build improvements and vcpkg integration.

- **vcpkg Integration**: Automatic FFmpeg installation and static linking on Windows/Linux/macOS
- **Rust 2024 Edition**: Updated to latest Rust edition with modern syntax
- **FFmpeg 8.0 Support**: Full support for FFmpeg 8.0 APIs with automatic version detection
- **Cross-platform Build Scripts**: build.ps1 (Windows), build.sh (Linux/macOS), build-mac.sh (macOS zsh)
- **Static Linking**: Configured for static linking with vcpkg-provided libraries via .cargo/config.toml
- **Visual Studio Setup**: Automatic MSVC environment configuration on Windows
- **Enhanced Examples**: New video-info tool, improved dump-frames with image export
- **Improved CI/CD**: Updated GitHub Actions workflows, modern action versions
- **Code Formatting**: Reformatted entire codebase with rustfmt (Rust 2024 edition)

Repository: https://github.com/ssoj13/playa-ffmpeg
Based on: ffmpeg-next 7.x (https://github.com/zmwangx/rust-ffmpeg)
Original: meh.'s rust-ffmpeg (https://github.com/meh/rust-ffmpeg)

---

5.0.0
-----

- Introduce conditional compilation flags to preserve functions that are
  removed from ffmpeg 5.0 and onwards.
- Fix examples so they are using the ffmpeg-sanctionned way of doing
  things. More specifically, AVStream.codec has been removed, and the
  correct way of getting the codec from a stream is to use
  Context::from_parameters(stream.parameters()) and then that context's
  encoder / decoder.

4.4.0
-----

- crate: `ffmpeg43` feature flag (noop since 4.3.4) has been dropped from default features.

- codec: deprecate APIs based on deprecated (since FFmpeg 3.1) `avcodec_decode_video2()` / `avcodec_decode_audio4()` / `avcodec_encode_video2()` /`avcodec_encode_audio2()` -- `decoder::Video::decode()`, `decode::Audio::decode()`, `encoder::Video::encode()` and `encoder::Audio::decode()`. Users should migrate to `send_packet()` / `send_eof()`, `receive_frame()`, `send_frame()` / `send_eof()`, and `receive_packet()` APIs instead, which are based on the modern send/receive APIs. See [documentation in `libavcodec/avcodec.h`](https://github.com/FFmpeg/FFmpeg/blob/n4.3.1/libavcodec/avcodec.h#L84-L196) for details. (#28)

- codec: fix signature of `Packet::write_interleaved`; previously `Result<bool, Error>`, now `Result<(), Error>`. (#25)

4.3.8
-----
- software::resampling: add Context::get_with for specifying additional options. (#41)

4.3.7
-----

- codec:  fix codec description potential null ptr issue. (#36)

4.3.6
-----

- util: fix Windows compatibility due to unavailable errnos. (#30)

4.3.5
-----

- util: add `util::log` module to expose FFmpeg's logging facilities.

- filter: add method `Source::close()` to expose `av_buffersrc_close`. (#23)

- codec: add new encoding/decoding APIs `send_frame()` / `send_eof()`, `receive_packet()` to `encoder::{Audio, Video}` and `send_packet()` / `send_eof()`, `receive_frame()` to `decoder::{Audio, Video}` based on modern send/receive APIs (instead of `avcodec_decode_video2()` / `avcodec_decode_audio4()` / `avcodec_encode_video2()` /`avcodec_encode_audio2()` which have been deprecated since FFmpeg 3.1). Users should consider switching to the new APIs. See [documentation in `libavcodec/avcodec.h`](https://github.com/FFmpeg/FFmpeg/blob/n4.3.1/libavcodec/avcodec.h#L84-L196) for details. (#28)

- util: introduce new `Error` variant `Error::Other { errno }` for wrapped POSIX error codes (see the `AVERROR` macro in `libavutil/error.h`), and reexport common POSIX error codes under `util::error`. (#24)

4.3.4
-----

- crate: FFmpeg version detection is now automatic, obseleting feature flags `ffmpeg4`, `ffmpeg41`, `ffmpeg42` and `ffmpeg43`. The flags are kept as noop for now, will be removed in 5.0.
