# TODO

1. Investigate timecode support implementation
2. Read EDL or OpenTimelineIO as input file
3. Explore OCIO/OIIO integration possibilities
4. Shotgrid integration
5. Explore headless operation - core without GUI, Python API only
6. Expose the entire application as a Python extension via PyO3 and maturin
7. Python API via RustPython - expose all major classes, widgets, dialogs, and core functionality
8. Make it possible to playback simple file/camera/text layers: using "default" parent comp that is becoming current and replacing it's contents with picked layer.
Technically we can make it like this for every other layer including Comp. Think on that, I need an opinion. Give me options.