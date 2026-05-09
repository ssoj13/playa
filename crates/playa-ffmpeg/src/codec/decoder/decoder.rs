use std::{
    ops::{Deref, DerefMut},
    ptr,
};

use super::{Audio, Check, Conceal, Opened, Subtitle, Video};
use crate::{
    Dictionary, Discard, Error, Rational,
    codec::{Context, traits},
    ffi::*,
};

/// A decoder for compressed media streams.
///
/// `Decoder` wraps an FFmpeg codec context configured for decoding. It provides methods
/// to open the decoder with various configurations and convert it to type-specific decoders
/// (video, audio, or subtitle).
///
/// # Lifecycle
///
/// 1. Create a `Decoder` from a codec context (typically from a stream)
/// 2. Open it with [`open()`](Decoder::open), [`open_as()`](Decoder::open_as), or type-specific methods
/// 3. Use the resulting [`Opened`] decoder to decode packets into frames
///
/// # Example
///
/// ```ignore
/// // Get decoder from stream codec parameters
/// let decoder = stream.codec().decoder();
///
/// // Open as video decoder
/// let mut video = decoder.video()?;
///
/// // Decode packets
/// video.send_packet(&packet)?;
/// let frame = video.receive_frame()?;
/// ```
pub struct Decoder(pub Context);

impl Decoder {
    /// Opens the decoder with default codec options.
    ///
    /// Uses the codec already associated with this decoder context (if any).
    ///
    /// # Errors
    ///
    /// Returns an error if the decoder cannot be opened (e.g., missing codec,
    /// invalid parameters, or resource allocation failure).
    pub fn open(mut self) -> Result<Opened, Error> {
        unsafe {
            // Call FFmpeg's avcodec_open2 with null codec (use context's codec) and null options
            match avcodec_open2(self.as_mut_ptr(), ptr::null(), ptr::null_mut()) {
                0 => Ok(Opened(self)),
                e => Err(Error::from(e)),
            }
        }
    }

    /// Opens the decoder with a specific codec.
    ///
    /// # Parameters
    ///
    /// * `codec` - The codec to use for decoding (must support decoding)
    ///
    /// # Errors
    ///
    /// Returns `Error::DecoderNotFound` if the codec doesn't support decoding,
    /// or other errors if the decoder cannot be opened.
    pub fn open_as<D: traits::Decoder>(mut self, codec: D) -> Result<Opened, Error> {
        unsafe {
            if let Some(codec) = codec.decoder() {
                match avcodec_open2(self.as_mut_ptr(), codec.as_ptr(), ptr::null_mut()) {
                    0 => Ok(Opened(self)),
                    e => Err(Error::from(e)),
                }
            } else {
                Err(Error::DecoderNotFound)
            }
        }
    }

    /// Opens the decoder with a specific codec and options dictionary.
    ///
    /// # Parameters
    ///
    /// * `codec` - The codec to use for decoding
    /// * `options` - Dictionary of codec-specific options (e.g., threading, quality settings)
    ///
    /// # Errors
    ///
    /// Returns `Error::DecoderNotFound` if the codec doesn't support decoding,
    /// or other errors if the decoder cannot be opened.
    ///
    /// # Note
    ///
    /// The options dictionary is consumed and reclaimed after the call.
    pub fn open_as_with<D: traits::Decoder>(mut self, codec: D, options: Dictionary) -> Result<Opened, Error> {
        unsafe {
            if let Some(codec) = codec.decoder() {
                // Disown the dictionary for FFmpeg to consume
                let mut opts = options.disown();
                let res = avcodec_open2(self.as_mut_ptr(), codec.as_ptr(), &mut opts);

                // Reclaim ownership to properly free the dictionary
                Dictionary::own(opts);

                match res {
                    0 => Ok(Opened(self)),
                    e => Err(Error::from(e)),
                }
            } else {
                Err(Error::DecoderNotFound)
            }
        }
    }

    /// Opens this decoder as a video decoder.
    ///
    /// Convenience method that finds an appropriate video codec, opens the decoder,
    /// and returns a type-specific video decoder.
    ///
    /// # Errors
    ///
    /// Returns `Error::DecoderNotFound` if no suitable video decoder is found for
    /// this context's codec ID.
    pub fn video(self) -> Result<Video, Error> {
        // Try to use codec already in context, otherwise find by ID
        if let Some(codec) = self.codec() {
            self.open_as(codec).and_then(|o| o.video())
        } else if let Some(codec) = super::find(self.id()) {
            self.open_as(codec).and_then(|o| o.video())
        } else {
            Err(Error::DecoderNotFound)
        }
    }

    /// Opens this decoder as an audio decoder.
    ///
    /// Convenience method that finds an appropriate audio codec, opens the decoder,
    /// and returns a type-specific audio decoder.
    ///
    /// # Errors
    ///
    /// Returns `Error::DecoderNotFound` if no suitable audio decoder is found for
    /// this context's codec ID.
    pub fn audio(self) -> Result<Audio, Error> {
        // Try to use codec already in context, otherwise find by ID
        if let Some(codec) = self.codec() {
            self.open_as(codec).and_then(|o| o.audio())
        } else if let Some(codec) = super::find(self.id()) {
            self.open_as(codec).and_then(|o| o.audio())
        } else {
            Err(Error::DecoderNotFound)
        }
    }

    /// Opens this decoder as a subtitle decoder.
    ///
    /// Convenience method that finds an appropriate subtitle codec, opens the decoder,
    /// and returns a type-specific subtitle decoder.
    ///
    /// # Errors
    ///
    /// Returns `Error::DecoderNotFound` if no suitable subtitle decoder is found for
    /// this context's codec ID.
    pub fn subtitle(self) -> Result<Subtitle, Error> {
        if let Some(codec) = super::find(self.id()) { self.open_as(codec).and_then(|o| o.subtitle()) } else { Err(Error::DecoderNotFound) }
    }

    /// Sets error concealment strategy.
    ///
    /// Configures how the decoder should handle corrupted data (e.g., missing macroblocks,
    /// broken slices). Higher concealment may produce visible artifacts but avoid decode failures.
    pub fn conceal(&mut self, value: Conceal) {
        unsafe {
            (*self.as_mut_ptr()).error_concealment = value.bits();
        }
    }

    /// Sets error detection/recognition level.
    ///
    /// Controls how strictly the decoder validates input data. Stricter checking catches
    /// more errors but may reject valid but non-compliant streams.
    pub fn check(&mut self, value: Check) {
        unsafe {
            (*self.as_mut_ptr()).err_recognition = value.bits();
        }
    }

    /// Sets which frames to skip during loop filtering (deblocking).
    ///
    /// Skipping loop filtering improves decode performance but reduces visual quality.
    /// Useful for low-power devices or when decoding for analysis rather than display.
    pub fn skip_loop_filter(&mut self, value: Discard) {
        unsafe {
            (*self.as_mut_ptr()).skip_loop_filter = value.into();
        }
    }

    /// Sets which frames to skip during IDCT (inverse DCT) processing.
    ///
    /// Skipping IDCT can significantly speed up decoding but produces lower quality output.
    /// Primarily useful for fast seeking or thumbnail extraction.
    pub fn skip_idct(&mut self, value: Discard) {
        unsafe {
            (*self.as_mut_ptr()).skip_idct = value.into();
        }
    }

    /// Sets which frames to skip entirely during decoding.
    ///
    /// Allows selective frame dropping (e.g., skip all B-frames, skip non-reference frames).
    /// Useful for fast playback or when only key frames are needed.
    pub fn skip_frame(&mut self, value: Discard) {
        unsafe {
            (*self.as_mut_ptr()).skip_frame = value.into();
        }
    }

    /// Gets the time base used for packet timestamps.
    ///
    /// This is the time unit for interpreting PTS/DTS values in input packets.
    /// Typically matches the stream's time base.
    pub fn packet_time_base(&self) -> Rational {
        unsafe { Rational::from((*self.as_ptr()).pkt_timebase) }
    }

    /// Sets the time base for packet timestamps.
    ///
    /// Must match the time base of packets being sent to this decoder.
    pub fn set_packet_time_base<R: Into<Rational>>(&mut self, value: R) {
        unsafe {
            (*self.as_mut_ptr()).pkt_timebase = value.into().into();
        }
    }
}

impl Deref for Decoder {
    type Target = Context;

    fn deref(&self) -> &<Self as Deref>::Target {
        &self.0
    }
}

impl DerefMut for Decoder {
    fn deref_mut(&mut self) -> &mut <Self as Deref>::Target {
        &mut self.0
    }
}

impl AsRef<Context> for Decoder {
    fn as_ref(&self) -> &Context {
        self
    }
}

impl AsMut<Context> for Decoder {
    fn as_mut(&mut self) -> &mut Context {
        &mut self.0
    }
}
