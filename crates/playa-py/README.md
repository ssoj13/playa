# playa-py

Python bindings for [playa](https://github.com/ssoj13/playa) - a fast image sequence player for VFX workflows.

## Installation

```bash
pip install playa
```

Or build from source:

```bash
cd playa
./bootstrap.ps1 python
```

## Usage

```python
import playa

# Run player with a file
playa.run(file="path/to/image.exr")

# Run with options
playa.run(
    file="path/to/sequence.0001.exr",
    autoplay=True,
    loop_playback=True,
    fullscreen=False,
    frame=0,
)

# Multiple files
playa.run(files=["file1.exr", "file2.exr", "file3.exr"])

# Get version
print(playa.version())
```

## Parameters

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `file` | str | None | Path to image file or sequence |
| `files` | list[str] | None | Additional files to load |
| `autoplay` | bool | False | Start playing immediately |
| `loop_playback` | bool | True | Enable loop mode |
| `fullscreen` | bool | False | Start in fullscreen mode |
| `frame` | int | None | Start at specific frame |
| `start` | int | None | Play range start frame |
| `end` | int | None | Play range end frame |

## Supported Formats

- **Images**: EXR, PNG, JPEG, TIFF, TGA, BMP, DPX
- **Video**: MP4, MOV, AVI (via FFmpeg)
- **Sequences**: Automatically detected from filename patterns

## License

MIT
