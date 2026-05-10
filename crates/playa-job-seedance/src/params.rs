//! Strongly-typed parameter helper for callers who prefer the type-safe path
//! over hand-building [`serde_json::Value`].
//!
//! The provider itself does not require this type — it forwards whatever
//! [`serde_json::Value`] arrived in [`playa_jobs::JobQueue::submit`] verbatim
//! to fal, letting fal own validation. Use this builder when constructing
//! params from Rust code (UI dialog, REST API, …).

use serde::{Deserialize, Serialize};

/// fal.ai `bytedance/seedance-2.0/image-to-video` request body.
///
/// Field reference: <https://fal.ai/models/bytedance/seedance-2.0/image-to-video>
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedanceImageToVideoParams {
    /// Motion description.
    pub prompt: String,
    /// Starting frame URL. Must be reachable from fal's servers (HTTPS,
    /// public, no auth). 30 MB max. Accepted formats: JPEG, PNG, WebP, GIF,
    /// AVIF.
    pub image_url: String,
    /// Optional ending frame for guided motion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_image_url: Option<String>,
    /// `"480p"`, `"720p"`, or `"1080p"`. fal returns 4xx on other values.
    #[serde(default = "default_resolution")]
    pub resolution: String,
    /// `Auto` or `Seconds(4..=15)`.
    #[serde(default)]
    pub duration: SeedanceDuration,
    /// `"auto"`, `"21:9"`, `"16:9"`, `"4:3"`, `"1:1"`, `"3:4"`, `"9:16"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aspect_ratio: Option<String>,
    /// Synchronised in-band audio. No extra cost.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generate_audio: Option<bool>,
    /// Reproducibility seed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
}

impl SeedanceImageToVideoParams {
    pub fn new(prompt: impl Into<String>, image_url: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            image_url: image_url.into(),
            end_image_url: None,
            resolution: default_resolution(),
            duration: SeedanceDuration::default(),
            aspect_ratio: None,
            generate_audio: None,
            seed: None,
        }
    }

    pub fn into_json(self) -> serde_json::Value {
        serde_json::to_value(self).expect("SeedanceImageToVideoParams is always JSON-serialisable")
    }
}

/// Duration is either the literal string `"auto"` or an integer in `4..=15`.
/// Custom (de)serialise so the wire form matches fal's expectation while
/// callers see a tight Rust enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeedanceDuration {
    Auto,
    Seconds(u8),
}

impl Default for SeedanceDuration {
    fn default() -> Self {
        Self::Auto
    }
}

impl Serialize for SeedanceDuration {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Auto => s.serialize_str("auto"),
            Self::Seconds(n) => s.serialize_u8(*n),
        }
    }
}

impl<'de> Deserialize<'de> for SeedanceDuration {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let v = serde_json::Value::deserialize(d)?;
        match v {
            serde_json::Value::String(s) if s == "auto" => Ok(Self::Auto),
            serde_json::Value::Number(n) => match n.as_u64() {
                Some(n) if (4..=15).contains(&n) => Ok(Self::Seconds(n as u8)),
                Some(n) => Err(D::Error::custom(format!(
                    "seedance duration must be 4..=15, got {n}"
                ))),
                None => Err(D::Error::custom("seedance duration must be u8")),
            },
            other => Err(D::Error::custom(format!(
                "seedance duration: expected \"auto\" or 4..=15 integer, got {other}"
            ))),
        }
    }
}

fn default_resolution() -> String {
    "720p".to_string()
}

// =============================================================================
// Text-to-video params — DIFFERENT shape from image-to-video on the wire.
// =============================================================================

/// fal.ai `bytedance/seedance-2.0/text-to-video` request body.
///
/// Notable differences from [`SeedanceImageToVideoParams`]:
/// - **No `image_url`.** Just text prompt.
/// - **`resolution` is `"480p"` or `"720p"` only.** No 1080p variant.
/// - **`duration` is a string** (`"auto"` or `"4"`..`"15"`), not an integer.
/// - **`generate_audio` defaults to `true`** server-side; sending `None` keeps
///   that default.
/// - Optional `end_user_id` for fal-side billing tracking.
///
/// Field reference: <https://fal.ai/models/bytedance/seedance-2.0/text-to-video>
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedanceTextToVideoParams {
    pub prompt: String,
    /// `"480p"` or `"720p"`. Default `"720p"`.
    #[serde(default = "default_resolution_t2v")]
    pub resolution: String,
    /// `"auto"` (default) or `"4"`..`"15"` as string.
    #[serde(default = "default_duration_t2v")]
    pub duration: String,
    /// `"auto"` (default), `"21:9"`, `"16:9"`, `"4:3"`, `"1:1"`, `"3:4"`, `"9:16"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aspect_ratio: Option<String>,
    /// fal default is `true` server-side; send `None` to honour it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generate_audio: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    /// fal end-user id for billing / abuse tracking (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_user_id: Option<String>,
}

impl SeedanceTextToVideoParams {
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            resolution: default_resolution_t2v(),
            duration: default_duration_t2v(),
            aspect_ratio: None,
            generate_audio: None,
            seed: None,
            end_user_id: None,
        }
    }

    pub fn into_json(self) -> serde_json::Value {
        serde_json::to_value(self).expect("SeedanceTextToVideoParams is always JSON-serialisable")
    }
}

fn default_resolution_t2v() -> String {
    "720p".to_string()
}

fn default_duration_t2v() -> String {
    "auto".to_string()
}

#[cfg(test)]
mod text_to_video_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn default_t2v_params_minimal_serialise() {
        let p = SeedanceTextToVideoParams::new("a story unfolds");
        let v = p.into_json();
        assert_eq!(v["prompt"], "a story unfolds");
        assert_eq!(v["resolution"], "720p");
        assert_eq!(v["duration"], "auto");
        // Optional fields skipped when None.
        assert!(v.get("aspect_ratio").is_none());
        assert!(v.get("generate_audio").is_none());
        assert!(v.get("seed").is_none());
        assert!(v.get("end_user_id").is_none());
        // No image_url field at all (vs image_to_video).
        assert!(v.get("image_url").is_none());
    }

    #[test]
    fn t2v_duration_is_string_per_fal_spec() {
        let p = SeedanceTextToVideoParams {
            duration: "5".into(),
            ..SeedanceTextToVideoParams::new("x")
        };
        let v = p.into_json();
        assert_eq!(v["duration"], "5"); // STRING, not integer.
        assert!(v["duration"].is_string());
    }

    #[test]
    fn t2v_full_round_trip() {
        let body = json!({
            "prompt": "drift",
            "resolution": "480p",
            "duration": "10",
            "aspect_ratio": "16:9",
            "generate_audio": false,
            "seed": 7,
            "end_user_id": "user-abc",
        });
        let p: SeedanceTextToVideoParams = serde_json::from_value(body.clone()).unwrap();
        assert_eq!(p.duration, "10");
        let back = p.into_json();
        assert_eq!(back, body);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn default_params_serialise_minimally() {
        let p = SeedanceImageToVideoParams::new("hello", "https://example.com/img.png");
        let v = p.into_json();
        assert_eq!(v["prompt"], "hello");
        assert_eq!(v["image_url"], "https://example.com/img.png");
        assert_eq!(v["resolution"], "720p");
        assert_eq!(v["duration"], "auto");
        assert!(v.get("end_image_url").is_none());
        assert!(v.get("aspect_ratio").is_none());
    }

    #[test]
    fn duration_seconds_serialise_as_integer() {
        let p = SeedanceImageToVideoParams {
            duration: SeedanceDuration::Seconds(8),
            ..SeedanceImageToVideoParams::new("x", "https://a/b.png")
        };
        let v = p.into_json();
        assert_eq!(v["duration"], 8);
    }

    #[test]
    fn duration_deserialises_auto_and_seconds() {
        let auto: SeedanceDuration = serde_json::from_str("\"auto\"").unwrap();
        assert_eq!(auto, SeedanceDuration::Auto);
        let n: SeedanceDuration = serde_json::from_str("12").unwrap();
        assert_eq!(n, SeedanceDuration::Seconds(12));
    }

    #[test]
    fn duration_rejects_out_of_range() {
        assert!(serde_json::from_str::<SeedanceDuration>("3").is_err());
        assert!(serde_json::from_str::<SeedanceDuration>("16").is_err());
        assert!(serde_json::from_str::<SeedanceDuration>("\"weird\"").is_err());
    }

    #[test]
    fn full_params_round_trip() {
        let body = json!({
            "prompt": "drift",
            "image_url": "https://x/i.jpg",
            "end_image_url": "https://x/o.jpg",
            "resolution": "1080p",
            "duration": 10,
            "aspect_ratio": "16:9",
            "generate_audio": true,
            "seed": 42,
        });
        let parsed: SeedanceImageToVideoParams = serde_json::from_value(body.clone()).unwrap();
        assert_eq!(parsed.duration, SeedanceDuration::Seconds(10));
        let back = parsed.into_json();
        assert_eq!(back, body);
    }
}
