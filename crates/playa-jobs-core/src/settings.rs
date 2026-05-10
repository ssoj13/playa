//! User-facing settings for the jobs subsystem.
//!
//! Lives in `playa-jobs-core` so external consumers (any host application
//! depending on the jobs facade) can persist these values without pulling
//! the host's `AppSettings` type in. The settings flow through the
//! pluggable prefs registry from `playa-prefs`: each app exposes a
//! "Jobs & Rendering" panel that renders against `&mut JobsSettings`.

use serde::{Deserialize, Serialize};

/// Persistent user preferences for the jobs subsystem.
///
/// All fields are persistent across application restarts (host serialises
/// the whole struct via its own settings storage). Values are read by:
/// - `JobQueue::submit` to enforce daily budget caps (when feature wired).
/// - The Jobs UI panel to drive per-job auto-attach / retention behaviour.
/// - The Preferences window's "Jobs & Rendering" panel for editing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobsSettings {
    /// When true, [`Self::daily_budget_usd`] caps cumulative cost in any
    /// single UTC day. Submits beyond the cap are rejected with
    /// `JobError::Provider("daily budget exceeded …")`.
    #[serde(default = "default_false")]
    pub daily_budget_enabled: bool,

    /// USD cap that applies when [`Self::daily_budget_enabled`] is true.
    /// Default 50.0 — high enough to not surprise a casual user, low
    /// enough to break a runaway loop.
    #[serde(default = "default_daily_budget_usd")]
    pub daily_budget_usd: f64,

    /// When a generation job completes, automatically attach the resulting
    /// mp4 as a new layer in the active comp. Default true — most users
    /// want to see the result immediately. Per-job override available
    /// in the Submit dialog.
    #[serde(default = "default_true")]
    pub auto_attach_mp4: bool,

    /// Auto-prune terminal jobs older than this many days. `None` = never
    /// prune (manual delete only). Default `Some(30)`.
    #[serde(default = "default_retention_days")]
    pub retention_days: Option<u32>,
}

impl Default for JobsSettings {
    fn default() -> Self {
        Self {
            daily_budget_enabled: default_false(),
            daily_budget_usd: default_daily_budget_usd(),
            auto_attach_mp4: default_true(),
            retention_days: default_retention_days(),
        }
    }
}

fn default_false() -> bool {
    false
}

fn default_true() -> bool {
    true
}

fn default_daily_budget_usd() -> f64 {
    50.0
}

fn default_retention_days() -> Option<u32> {
    Some(30)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sensible() {
        let s = JobsSettings::default();
        assert!(!s.daily_budget_enabled, "off by default — no surprise cap");
        assert_eq!(s.daily_budget_usd, 50.0);
        assert!(s.auto_attach_mp4, "users see result immediately by default");
        assert_eq!(s.retention_days, Some(30));
    }

    #[test]
    fn json_round_trip_with_all_fields() {
        let original = JobsSettings {
            daily_budget_enabled: true,
            daily_budget_usd: 12.34,
            auto_attach_mp4: false,
            retention_days: None,
        };
        let json = serde_json::to_string(&original).unwrap();
        let restored: JobsSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, original);
    }

    #[test]
    fn deserialize_with_missing_fields_uses_defaults() {
        // Partial save (e.g. older version with fewer fields) must not panic.
        let partial = "{}";
        let restored: JobsSettings = serde_json::from_str(partial).unwrap();
        assert_eq!(restored, JobsSettings::default());

        let one_field = r#"{"daily_budget_enabled": true}"#;
        let restored: JobsSettings = serde_json::from_str(one_field).unwrap();
        assert!(restored.daily_budget_enabled);
        // Other fields fall back to defaults.
        assert_eq!(restored.daily_budget_usd, 50.0);
        assert!(restored.auto_attach_mp4);
    }

    #[test]
    fn retention_none_round_trips() {
        let s = JobsSettings {
            retention_days: None,
            ..JobsSettings::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        let restored: JobsSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.retention_days, None);
    }
}
