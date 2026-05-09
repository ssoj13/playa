use std::{
    ops::{Deref, DerefMut},
    ptr,
};

use crate::ffi::*;
use libc::c_int;

use super::{audio, subtitle, video};
use crate::{Error, Frame, codec::Context, media, packet};

/// An encoder for compressing raw media frames.
///
/// `Encoder` wraps an FFmpeg codec context configured for encoding. It provides methods
/// to configure encoding parameters and convert it to type-specific encoders (video, audio,
/// or subtitle).
///
/// # Lifecycle
///
/// 1. Create an `Encoder` from a codec
/// 2. Configure encoding parameters (bitrate, quality, etc.)
/// 3. Convert to type-specific encoder with [`video()`](Encoder::video), [`audio()`](Encoder::audio), or [`subtitle()`](Encoder::subtitle)
/// 4. Send raw frames with [`send_frame()`](Encoder::send_frame) and receive encoded packets
///
/// # Example
///
/// ```ignore
/// // Create encoder for H.264
/// let codec = encoder::find(Id::H264).unwrap();
/// let mut encoder = codec.encoder().unwrap();
///
/// // Configure as video encoder
/// let mut video = encoder.video()?;
/// video.set_width(1920);
/// video.set_height(1080);
/// video.set_format(Pixel::YUV420P);
/// video.set_bit_rate(4_000_000);
///
/// // Encode frames
/// video.send_frame(&frame)?;
/// video.receive_packet(&mut packet)?;
/// ```
pub struct Encoder(pub Context);

impl Encoder {
    /// Converts this encoder to a video encoder.
    ///
    /// If the context media type is Unknown, it will be set to Video.
    ///
    /// # Errors
    ///
    /// Returns `Error::InvalidData` if the context is already configured for a
    /// non-video media type.
    pub fn video(mut self) -> Result<video::Video, Error> {
        match self.medium() {
            media::Type::Unknown => {
                unsafe {
                    // Set codec type to video
                    (*self.as_mut_ptr()).codec_type = media::Type::Video.into();
                }

                Ok(video::Video(self))
            }

            media::Type::Video => Ok(video::Video(self)),

            _ => Err(Error::InvalidData),
        }
    }

    /// Converts this encoder to an audio encoder.
    ///
    /// If the context media type is Unknown, it will be set to Audio.
    ///
    /// # Errors
    ///
    /// Returns `Error::InvalidData` if the context is already configured for a
    /// non-audio media type.
    pub fn audio(mut self) -> Result<audio::Audio, Error> {
        match self.medium() {
            media::Type::Unknown => {
                unsafe {
                    // Set codec type to audio
                    (*self.as_mut_ptr()).codec_type = media::Type::Audio.into();
                }

                Ok(audio::Audio(self))
            }

            media::Type::Audio => Ok(audio::Audio(self)),

            _ => Err(Error::InvalidData),
        }
    }

    /// Converts this encoder to a subtitle encoder.
    ///
    /// If the context media type is Unknown, it will be set to Subtitle.
    ///
    /// # Errors
    ///
    /// Returns `Error::InvalidData` if the context is already configured for a
    /// non-subtitle media type.
    pub fn subtitle(mut self) -> Result<subtitle::Subtitle, Error> {
        match self.medium() {
            media::Type::Unknown => {
                unsafe {
                    // Set codec type to subtitle
                    (*self.as_mut_ptr()).codec_type = media::Type::Subtitle.into();
                }

                Ok(subtitle::Subtitle(self))
            }

            media::Type::Subtitle => Ok(subtitle::Subtitle(self)),

            _ => Err(Error::InvalidData),
        }
    }

    /// Sends a raw frame to the encoder.
    ///
    /// The encoder will buffer and encode the frame according to its configuration.
    /// Call [`receive_packet()`](Encoder::receive_packet) to retrieve encoded packets.
    ///
    /// # Errors
    ///
    /// - `Error::Other(EAGAIN)` - The encoder needs more frames before producing output
    /// - `Error::Eof` - The encoder has been flushed and won't accept more frames
    /// - Other errors indicate encoding failure
    pub fn send_frame(&mut self, frame: &Frame) -> Result<(), Error> {
        unsafe {
            match avcodec_send_frame(self.as_mut_ptr(), frame.as_ptr()) {
                e if e < 0 => Err(Error::from(e)),
                _ => Ok(()),
            }
        }
    }

    /// Signals end-of-stream and enters draining mode.
    ///
    /// After calling this, continue calling [`receive_packet()`](Encoder::receive_packet)
    /// until it returns `Error::Eof` to retrieve all buffered encoded packets.
    ///
    /// # Errors
    ///
    /// Returns an error if the encoder is in an invalid state.
    pub fn send_eof(&mut self) -> Result<(), Error> {
        unsafe {
            // Send null frame to signal EOF
            self.send_frame(&Frame::wrap(ptr::null_mut()))
        }
    }

    /// Receives an encoded packet from the encoder.
    ///
    /// Call this repeatedly after [`send_frame()`](Encoder::send_frame) to retrieve
    /// all available encoded packets.
    ///
    /// # Errors
    ///
    /// - `Error::Other(EAGAIN)` - Need to send more frames before output is available
    /// - `Error::Eof` - No more packets (encoder has been drained)
    /// - Other errors indicate encoding failure
    pub fn receive_packet<P: packet::Mut>(&mut self, packet: &mut P) -> Result<(), Error> {
        unsafe {
            match avcodec_receive_packet(self.as_mut_ptr(), packet.as_mut_ptr()) {
                e if e < 0 => Err(Error::from(e)),
                _ => Ok(()),
            }
        }
    }

    /// Sets the target bitrate in bits per second.
    ///
    /// This is the average bitrate the encoder will try to achieve. Used for
    /// CBR (constant bitrate) or ABR (average bitrate) encoding.
    pub fn set_bit_rate(&mut self, value: usize) {
        unsafe {
            (*self.as_mut_ptr()).bit_rate = value as i64;
        }
    }

    /// Sets the maximum bitrate in bits per second for VBR encoding.
    ///
    /// Combined with bitrate, this defines the bitrate range for variable bitrate encoding.
    /// The encoder will never exceed this bitrate.
    pub fn set_max_bit_rate(&mut self, value: usize) {
        unsafe {
            (*self.as_mut_ptr()).rc_max_rate = value as i64;
        }
    }

    /// Sets the bitrate tolerance for rate control.
    ///
    /// Defines how much the bitrate can deviate from the target. Higher values
    /// allow more variation (better quality in complex scenes) but less consistent bitrate.
    pub fn set_tolerance(&mut self, value: usize) {
        unsafe {
            (*self.as_mut_ptr()).bit_rate_tolerance = value as c_int;
        }
    }

    /// Sets the global quality/quantizer for quality-based encoding.
    ///
    /// Interpretation is codec-specific. For example:
    /// - MPEG-2/4: Use 1 (best) to 31 (worst)
    /// - H.264: Use with CRF mode
    ///
    /// Lower values = higher quality = larger file size.
    pub fn set_quality(&mut self, value: usize) {
        unsafe {
            (*self.as_mut_ptr()).global_quality = value as c_int;
        }
    }

    /// Sets the compression level.
    ///
    /// Codec-specific parameter controlling the speed/compression tradeoff.
    /// Higher values = slower encoding but better compression.
    ///
    /// Pass `None` to use the codec's default (-1).
    pub fn set_compression(&mut self, value: Option<usize>) {
        unsafe {
            if let Some(value) = value {
                (*self.as_mut_ptr()).compression_level = value as c_int;
            } else {
                // -1 means use codec default
                (*self.as_mut_ptr()).compression_level = -1;
            }
        }
    }
}

impl Deref for Encoder {
    type Target = Context;

    fn deref(&self) -> &<Self as Deref>::Target {
        &self.0
    }
}

impl DerefMut for Encoder {
    fn deref_mut(&mut self) -> &mut <Self as Deref>::Target {
        &mut self.0
    }
}

impl AsRef<Context> for Encoder {
    fn as_ref(&self) -> &Context {
        self
    }
}

impl AsMut<Context> for Encoder {
    fn as_mut(&mut self) -> &mut Context {
        &mut *self
    }
}
