use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use crate::providers::Message;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCache {
    pub conversation: Vec<Message>,
    pub provider: Option<String>,
    pub model: Option<String>,
}

pub fn load() -> Option<SessionCache> {
    let path = session_path();
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

pub fn save(cache: &SessionCache) {
    let path = session_path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    // Filter out system messages except Ouracle session markers (which are
    // needed to resume a session across launches). Scaffold context is
    // reloaded fresh from CWD files each time.
    let filtered = SessionCache {
        conversation: cache
            .conversation
            .iter()
            .filter(|m| {
                m.role != crate::providers::Role::System
                    || m.content.starts_with("ouracle:session:")
            })
            .cloned()
            .collect(),
        provider: cache.provider.clone(),
        model: cache.model.clone(),
    };
    if let Ok(raw) = serde_json::to_string_pretty(&filtered) {
        let _ = fs::write(path, raw);
    }
}

fn session_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let dir = PathBuf::from(home).join(".ripl").join("sessions");
    let hash = project_hash();
    dir.join(format!("{}.json", hash))
}

fn project_hash() -> u64 {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut hasher = DefaultHasher::new();
    cwd.to_string_lossy().hash(&mut hasher);
    hasher.finish()
}
