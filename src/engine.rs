use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use regex::RegexSet;

use crate::config::{Action, Config};
use crate::log;

/// A compiled rule set. All patterns are compiled into a single `RegexSet`, so
/// matching a URL is one pass regardless of how many rules exist.
pub struct Engine {
    set: RegexSet,
    rules: Vec<CompiledRule>,
    default: Action,
    path: PathBuf,
    mtime: Option<SystemTime>,
}

struct CompiledRule {
    name: Option<String>,
    action: Action,
}

impl Engine {
    pub fn load(path: &Path) -> Engine {
        match Self::try_load(path) {
            Ok(engine) => {
                log::log(&format!(
                    "loaded {} rule(s) from {}",
                    engine.rules.len(),
                    path.display()
                ));
                engine
            }
            Err(err) => {
                log::log(&format!("config error ({err}); falling back to Chrome"));
                Engine::fallback(path)
            }
        }
    }

    fn fallback(path: &Path) -> Engine {
        Engine {
            set: RegexSet::empty(),
            rules: Vec::new(),
            default: Action::default(),
            path: path.to_path_buf(),
            mtime: None,
        }
    }

    fn try_load(path: &Path) -> Result<Engine, String> {
        let text =
            fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let config: Config = serde_json::from_str(&text)
            .map_err(|e| format!("invalid JSON in {}: {e}", path.display()))?;

        let patterns: Vec<String> = config.rules.iter().map(|r| r.r#match.clone()).collect();
        let set = RegexSet::new(&patterns).map_err(|e| format!("invalid regex: {e}"))?;

        let rules = config
            .rules
            .into_iter()
            .map(|r| CompiledRule {
                name: r.name,
                action: r.action,
            })
            .collect();

        let mtime = fs::metadata(path).and_then(|m| m.modified()).ok();

        Ok(Engine {
            set,
            rules,
            default: config.default,
            path: path.to_path_buf(),
            mtime,
        })
    }

    /// First matching rule wins; otherwise the default action.
    pub fn resolve(&self, url: &str) -> (&Action, Option<&str>) {
        if let Some(index) = self.set.matches(url).into_iter().next() {
            let rule = &self.rules[index];
            (&rule.action, rule.name.as_deref())
        } else {
            (&self.default, None)
        }
    }

    /// Cheap hot-reload: only re-reads the file when its mtime changed.
    pub fn reload_if_changed(&mut self) {
        let mtime = fs::metadata(&self.path).and_then(|m| m.modified()).ok();
        if mtime == self.mtime {
            return;
        }
        match Self::try_load(&self.path) {
            Ok(engine) => {
                log::log("config changed; reloaded");
                *self = engine;
            }
            Err(err) => {
                log::log(&format!(
                    "config reload failed ({err}); keeping previous rules"
                ));
                self.mtime = mtime;
            }
        }
    }
}
