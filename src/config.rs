use serde::Deserialize;

/// Top level of `~/.config/urwhere.json`.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// Where unmatched URLs go. Defaults to Chrome's current profile.
    #[serde(default)]
    pub default: Action,

    /// Ordered rules. The first rule whose regex matches the URL wins.
    #[serde(default)]
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Rule {
    /// Optional human readable name, shown by `urwhere --test` and in logs.
    #[serde(default)]
    pub name: Option<String>,

    /// A regular expression tested against the full URL (e.g.
    /// `(?i)^https?://(?:www\.)?github\.com/zephyrcloudio(?:/|$)`).
    pub r#match: String,

    #[serde(flatten)]
    pub action: Action,
}

/// Where a URL should open.
#[derive(Debug, Clone, Deserialize)]
pub struct Action {
    /// Application name or path, e.g. `Google Chrome`.
    #[serde(default = "default_browser")]
    pub browser: String,

    /// Chrome profile *name* as shown in the profile picker (e.g. `Zephyr`),
    /// or the on-disk directory name (e.g. `Profile 1`).
    #[serde(default)]
    pub profile: Option<String>,
}

fn default_browser() -> String {
    "Google Chrome".to_string()
}

impl Default for Action {
    fn default() -> Self {
        Self {
            browser: default_browser(),
            profile: None,
        }
    }
}
