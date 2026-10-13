//! App manifests: how Settings finds Yozora apps without knowing about them in advance.
//!
//! Each app installs `yozora/apps/<id>.toml` under a data directory. A user manifest in
//! `$XDG_DATA_HOME` overrides a system one with the same id.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::paths;

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct AppManifest {
    pub id: String,
    pub name: String,
    pub binary: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// Config file name inside `~/.config/yozora/`.
    pub config: String,
    /// Settings schema file name, next to the manifest.
    #[serde(default)]
    pub settings: Option<String>,
    /// Control socket name inside `$XDG_RUNTIME_DIR/yozora/`.
    #[serde(default)]
    pub socket: Option<String>,

    /// Where this manifest was found. Filled in on load.
    #[serde(skip)]
    pub dir: PathBuf,
}

impl AppManifest {
    pub fn schema_path(&self) -> Option<PathBuf> {
        self.settings.as_ref().map(|s| self.dir.join(s))
    }

    pub fn config_path(&self) -> PathBuf {
        paths::config_dir().join(&self.config)
    }

    pub fn socket_path(&self) -> Option<PathBuf> {
        self.socket.as_ref().map(|s| paths::runtime_dir().join(s))
    }
}

/// Finds every installed app, in a stable order: by name.
pub fn discover() -> Vec<AppManifest> {
    discover_in(&paths::manifest_dirs())
}

pub fn discover_in(dirs: &[PathBuf]) -> Vec<AppManifest> {
    let mut found: Vec<AppManifest> = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut paths: Vec<_> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "toml"))
            // Settings schemas sit next to manifests.
            .filter(|p| !p.to_string_lossy().ends_with(".settings.toml"))
            .collect();
        paths.sort();
        for path in paths {
            match load(&path) {
                // Earlier directories win.
                Ok(manifest) if !found.iter().any(|m| m.id == manifest.id) => found.push(manifest),
                Ok(_) => {}
                Err(e) => tracing::warn!("skipping app manifest {}: {e:#}", path.display()),
            }
        }
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found
}

fn load(path: &Path) -> anyhow::Result<AppManifest> {
    let text = std::fs::read_to_string(path)?;
    let mut manifest: AppManifest = toml::from_str(&text)?;
    manifest.dir = path.parent().unwrap_or(Path::new("/")).to_path_buf();
    anyhow::ensure!(
        !manifest.id.is_empty() && manifest.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
        "id must be letters, digits and dashes"
    );
    anyhow::ensure!(!manifest.config.contains('/'), "config must be a file name");
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_manifests_override_system_ones() {
        let root = std::env::temp_dir().join(format!("yozora-manifest-{}", std::process::id()));
        let user = root.join("user");
        let system = root.join("system");
        std::fs::create_dir_all(&user).unwrap();
        std::fs::create_dir_all(&system).unwrap();
        let bar =
            |name: &str| format!("id = \"bar\"\nname = \"{name}\"\nbinary = \"yozora-bar\"\nconfig = \"bar.toml\"\n");
        std::fs::write(user.join("bar.toml"), bar("User Bar")).unwrap();
        std::fs::write(system.join("bar.toml"), bar("System Bar")).unwrap();
        std::fs::write(
            system.join("launch.toml"),
            "id = \"launch\"\nname = \"Yozora Launch\"\nbinary = \"yozora-launch\"\nconfig = \"launch.toml\"\nsettings = \"launch.settings.toml\"\n",
        )
        .unwrap();
        std::fs::write(system.join("broken.toml"), "id = 3").unwrap();
        std::fs::write(system.join("launch.settings.toml"), "[[section]]\ntitle = \"x\"\n").unwrap();

        let apps = discover_in(&[user, system.clone()]);
        assert_eq!(apps.len(), 2);
        assert_eq!(apps[0].name, "User Bar");
        assert_eq!(apps[1].schema_path(), Some(system.join("launch.settings.toml")));
        let _ = std::fs::remove_dir_all(&root);
    }
}
