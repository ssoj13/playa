use image::{ImageBuffer, Rgb};
/// Simple video file analyzer
///
/// Usage:
///   cargo run --example video-info -- ls                    # List all codecs
///   cargo run --example video-info -- <video-file>          # Analyze video
///   cargo run --example video-info -- <video-file> <dir>    # Analyze + save frames
///
/// Shows:
/// - File metadata (duration, bitrate, format)
/// - Stream information (codecs, resolution, fps)
/// - Frame count estimation
/// - First frame decoding test
/// - Dumps first 10 frames to JPEG files (optional)
use playa_ffmpeg as ffmpeg;
use std::{env, fs, path::Path, ptr};

fn list_codecs() {
    println!("=== FFmpeg Available Codecs ===\n");

    let mut video_decoders = Vec::new();
    let mut audio_decoders = Vec::new();
    let mut video_encoders = Vec::new();
    let mut audio_encoders = Vec::new();

    // Use FFI to iterate through all codecs
    unsafe {
        let mut opaque: *mut std::ffi::c_void = ptr::null_mut();

        loop {
            let codec_ptr = ffmpeg::ffi::av_codec_iterate(&mut opaque as *mut *mut std::ffi::c_void);
            if codec_ptr.is_null() {
                break;
            }

            let codec = ffmpeg::Codec::wrap(codec_ptr);
            let medium = codec.medium();
            let name = codec.name().to_string();
            let desc = codec.description().to_string();

            match medium {
                ffmpeg::media::Type::Video => {
                    if codec.is_decoder() {
                        video_decoders.push((name.clone(), desc.clone()));
                    }
                    if codec.is_encoder() {
                        video_encoders.push((name, desc));
                    }
                }
                ffmpeg::media::Type::Audio => {
                    if codec.is_decoder() {
                        audio_decoders.push((name.clone(), desc.clone()));
                    }
                    if codec.is_encoder() {
                        audio_encoders.push((name, desc));
                    }
                }
                _ => {}
            }
        }
    }

    // Sort by name
    video_decoders.sort_by(|a, b| a.0.cmp(&b.0));
    audio_decoders.sort_by(|a, b| a.0.cmp(&b.0));
    video_encoders.sort_by(|a, b| a.0.cmp(&b.0));
    audio_encoders.sort_by(|a, b| a.0.cmp(&b.0));

    // Print decoders
    println!("📥 DECODERS\n");

    println!("Video Decoders ({}):", video_decoders.len());
    for (name, desc) in &video_decoders {
        println!("  {:20} - {}", name, desc);
    }
    println!();

    println!("Audio Decoders ({}):", audio_decoders.len());
    for (name, desc) in &audio_decoders {
        println!("  {:20} - {}", name, desc);
    }
    println!();

    // Print encoders
    println!("📤 ENCODERS\n");

    println!("Video Encoders ({}):", video_encoders.len());
    for (name, desc) in &video_encoders {
        println!("  {:20} - {}", name, desc);
    }
    println!();

    println!("Audio Encoders ({}):", audio_encoders.len());
    for (name, desc) in &audio_encoders {
        println!("  {:20} - {}", name, desc);
    }
    println!();

    // Summary
    println!("📊 SUMMARY");
    println!("  Total Video Decoders: {}", video_decoders.len());
    println!("  Total Audio Decoders: {}", audio_decoders.len());
    println!("  Total Video Encoders: {}", video_encoders.len());
    println!("  Total Audio Encoders: {}", audio_encoders.len());
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize FFmpeg
    ffmpeg::init()?;

    // Get filename from args
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <video-file|ls>", args[0]);
        eprintln!("\nExamples:");
        eprintln!("  {} ls                    # List all available codecs", args[0]);
        eprintln!("  {} sample.mp4            # Analyze video file", args[0]);
        eprintln!("  {} sample.mp4 ./frames   # Analyze + save frames", args[0]);
        std::process::exit(1);
    }

    let input_file = &args[1];

    // Check for 'ls' command
    if input_file == "ls" {
        list_codecs();
        return Ok(());
    }

    println!("=== FFmpeg Video Analyzer ===\n");
    println!("File: {}\n", input_file);

    // Open input file
    let ictx = ffmpeg::format::input(&input_file)?;

    // === FILE METADATA ===
    println!("📄 FILE METADATA");
    println!("  Format: {}", ictx.format().name());
    println!("  Format (long): {}", ictx.format().description());

    let duration = ictx.duration();
    if duration > 0 {
        let secs = duration as f64 / f64::from(ffmpeg::ffi::AV_TIME_BASE);
        println!("  Duration: {:.2}s ({:.2} min)", secs, secs / 60.0);
    }

    let bitrate = ictx.bit_rate();
    if bitrate > 0 {
        println!("  Bitrate: {:.2} Mbps", bitrate as f64 / 1_000_000.0);
    }

    // === METADATA TAGS ===
    let metadata = ictx.metadata();
    if metadata.iter().count() > 0 {
        println!("\n  Tags:");
        for (key, value) in metadata.iter() {
            println!("    {}: {}", key, value);
        }
    }

    // === STREAMS INFO ===
    println!("\n📺 STREAMS ({} total)", ictx.nb_streams());

    let mut video_stream_index = None;

    for stream in ictx.streams() {
        let codec_params = stream.parameters();
        let media_type = codec_params.medium();

        println!("\n  Stream #{}", stream.index());
        println!("    Type: {:?}", media_type);
        println!("    Codec: {:?}", codec_params.id());
        println!("    Time base: {}/{}", stream.time_base().numerator(), stream.time_base().denominator());

        let fps = stream.avg_frame_rate();
        if fps.numerator() > 0 {
            println!("    FPS: {:.2}", fps.numerator() as f64 / fps.denominator() as f64);
        }

        match media_type {
            ffmpeg::media::Type::Video => {
                video_stream_index = Some(stream.index());

                // Video-specific info
                if let Ok(video) = ffmpeg::codec::context::Context::from_parameters(codec_params) {
                    let video = video.decoder().video()?;
                    println!("    Resolution: {}x{}", video.width(), video.height());
                    println!("    Pixel format: {:?}", video.format());

                    let aspect = video.aspect_ratio();
                    if aspect.numerator() > 0 {
                        println!("    Aspect ratio: {}/{} ({:.2})", aspect.numerator(), aspect.denominator(), aspect.numerator() as f64 / aspect.denominator() as f64);
                    }
                }
            }
            ffmpeg::media::Type::Audio => {
                if let Ok(audio) = ffmpeg::codec::context::Context::from_parameters(codec_params) {
                    let audio = audio.decoder().audio()?;
                    println!("    Sample rate: {} Hz", audio.rate());
                    println!("    Channels: {}", audio.channels());
                    println!("    Format: {:?}", audio.format());
                }
            }
            ffmpeg::media::Type::Subtitle => {
                println!("    (Subtitle stream)");
            }
            _ => {}
        }

        // Stream metadata
        let stream_meta = stream.metadata();
        if stream_meta.iter().count() > 0 {
            println!("    Metadata:");
            for (key, value) in stream_meta.iter() {
                println!("      {}: {}", key, value);
            }
        }
    }

    // === FRAME COUNT ESTIMATION ===
    if let Some(stream_idx) = video_stream_index {
        let stream = ictx.stream(stream_idx).unwrap();

        let duration = stream.duration();
        let fps = stream.avg_frame_rate();

        if duration > 0 && fps.numerator() > 0 {
            let tb = stream.time_base();
            let duration_secs = duration as f64 * tb.numerator() as f64 / tb.denominator() as f64;
            let frame_rate = fps.numerator() as f64 / fps.denominator() as f64;
            let estimated_frames = (duration_secs * frame_rate) as u64;

            println!("\n📊 FRAME INFO");
            println!("  Estimated frames: ~{}", estimated_frames);
        }
    }

    // === DECODE AND SAVE FRAMES ===
    if let Some(stream_idx) = video_stream_index {
        println!("\n🎬 FRAME DECODING TEST");

        let input = ictx.stream(stream_idx).unwrap();
        let codec_params = input.parameters();

        let mut decoder = ffmpeg::codec::context::Context::from_parameters(codec_params)?.decoder().video()?;

        let width = decoder.width();
        let height = decoder.height();

        // Setup output directory for frames
        let output_dir = if args.len() > 2 { args[2].clone() } else { "./frames".to_string() };

        // Create scaler for YUV420P -> RGB24 conversion
        let mut scaler = ffmpeg::software::scaling::Context::get(decoder.format(), width, height, ffmpeg::format::Pixel::RGB24, width, height, ffmpeg::software::scaling::Flags::BILINEAR)?;

        let mut ictx = ffmpeg::format::input(&input_file)?;
        let mut frames_saved = 0;
        const MAX_FRAMES: usize = 10;

        for (stream, packet) in ictx.packets() {
            if stream.index() == stream_idx {
                decoder.send_packet(&packet)?;

                let mut decoded = ffmpeg::util::frame::video::Video::empty();
                while decoder.receive_frame(&mut decoded).is_ok() {
                    if frames_saved == 0 {
                        println!("  ✓ Successfully decoded first frame!");
                        println!("    Width: {}", decoded.width());
                        println!("    Height: {}", decoded.height());
                        println!("    Format: {:?}", decoded.format());
                        println!("    PTS: {:?}", decoded.pts());
                    }

                    // Convert YUV -> RGB
                    let mut rgb_frame = ffmpeg::util::frame::video::Video::empty();
                    scaler.run(&decoded, &mut rgb_frame)?;

                    // Save frame as JPEG
                    if frames_saved < MAX_FRAMES {
                        // Create output directory if doesn't exist
                        if frames_saved == 0 {
                            fs::create_dir_all(&output_dir)?;
                            println!("\n  📁 Saving frames to: {}/", output_dir);
                        }

                        let output_path = Path::new(&output_dir).join(format!("frame_{:03}.jpg", frames_saved + 1));

                        // Get RGB data from frame
                        let rgb_data = rgb_frame.data(0);
                        let stride = rgb_frame.stride(0);

                        // Create ImageBuffer (handle stride != width*3 case)
                        let mut img_data = Vec::with_capacity((width * height * 3) as usize);
                        for y in 0..height {
                            let row_start = (y * stride as u32) as usize;
                            let row_end = row_start + (width * 3) as usize;
                            img_data.extend_from_slice(&rgb_data[row_start..row_end]);
                        }

                        let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_raw(width, height, img_data).ok_or("Failed to create image buffer")?;

                        img.save(&output_path)?;
                        println!("  ✓ Saved frame {}/{}: {}", frames_saved + 1, MAX_FRAMES, output_path.display());

                        frames_saved += 1;
                        if frames_saved >= MAX_FRAMES {
                            break;
                        }
                    }
                }

                if frames_saved >= MAX_FRAMES {
                    break;
                }
            }
        }

        if frames_saved == 0 {
            println!("  ✗ Failed to decode any frames");
        } else {
            println!("\n  ✅ Total frames saved: {}", frames_saved);
        }
    } else {
        println!("\n⚠ No video stream found");
    }

    println!("\n✅ Analysis complete!");

    Ok(())
}
