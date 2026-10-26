//! App settings (`~/.config/kith/settings.json`): sorting, theme, field templates, Google account.

use crate::fields::{builtin_templates, Template};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// "family" or "given"
    pub sort_by: String,
    /// "given" (Jane Doe) or "family" (Doe, Jane)
    pub display_order: String,
    /// "system", "light" or "dark"
    pub theme: String,
    pub active_template: String,
    /// Your own templates (the built-in ones are added by `templates()`).
    pub custom_templates: Vec<Template>,
    /// Fields shown on top of the template for this person; kept per template name.
    pub google: GoogleSettings,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct GoogleSettings {
    /// OAuth client of your own Google Cloud project (see docs/google-setup.md).
    pub client_id: String,
    pub client_secret: String,
    /// The account's e-mail address once signed in.
    pub account: String,
    /// Changes made in Kith are sent to Google. Off until you have checked a first read-only sync.
    pub two_way: bool,
    /// With `two_way`: contacts deleted in Kith are deleted on Google too (Google keeps them 30 days in its trash).
    pub allow_deletes: bool,
    /// A manual two-way sync has been run: only then may the automatic sync write to Google.
    pub two_way_confirmed: bool,
    pub auto_sync_minutes: u32,
    pub last_sync: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            sort_by: "family".into(),
            display_order: "given".into(),
            theme: "system".into(),
            active_template: "Personal".into(),
            custom_templates: vec![],
            google: GoogleSettings::default(),
        }
    }
}

impl Settings {
    pub fn path() -> PathBuf {
        let base = std::env::var("XDG_CONFIG_HOME").ok().filter(|x| !x.is_empty()).map(PathBuf::from).unwrap_or_else(|| {
            PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".config")
        });
        base.join("kith").join("settings.json")
    }

    pub fn load() -> Settings {
        std::fs::read_to_string(Self::path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let p = Self::path();
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        let tmp = p.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self).unwrap_or_default())?;
        // the file holds the OAuth client secret: owner only
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
        }
        std::fs::rename(tmp, p)
    }

    pub fn templates(&self) -> Vec<Template> {
        let mut v = builtin_templates();
        v.extend(self.custom_templates.iter().cloned());
        v
    }
}
