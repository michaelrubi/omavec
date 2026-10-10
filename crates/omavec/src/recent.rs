//! The documents opened lately, newest first, kept in
//! `~/.config/omavec/recent.toml`.

use std::path::{Path, PathBuf};

/// How many are remembered.
const MOST: usize = 10;

#[derive(Default)]
pub struct Recent {
    files: Vec<PathBuf>,
    /// Where the list is kept; `None` in tests, which keep it nowhere.
    store: Option<PathBuf>,
}

/// Omavec's folder under `kind`'s base: `XDG_CONFIG_HOME` or `~/.config`
/// for settings, `XDG_STATE_HOME` or `~/.local/state` for what it is in the
/// middle of.
pub fn home(variable: &str, fallback: &str) -> Option<PathBuf> {
    let base = std::env::var_os(variable).map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(fallback)))?;
    Some(base.join("omavec"))
}

impl Recent {
    /// The list as it was left, without what is no longer there.
    pub fn load(store: PathBuf) -> Self {
        let text = std::fs::read_to_string(&store).unwrap_or_default();
        let table: toml::Table = text.parse().unwrap_or_default();
        let listed = table.get("files").and_then(|files| files.as_array()).into_iter().flatten().filter_map(|file| file.as_str());
        let mut files: Vec<PathBuf> = Vec::new();
        for file in listed.map(PathBuf::from) {
            if file.exists() && !files.contains(&file) && files.len() < MOST {
                files.push(file);
            }
        }
        Self { files, store: Some(store) }
    }

    pub fn files(&self) -> &[PathBuf] {
        &self.files
    }

    /// Puts `path` at the top of the list.
    pub fn add(&mut self, path: &Path) {
        let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        self.files.retain(|file| *file != path);
        self.files.insert(0, path);
        self.files.truncate(MOST);
        self.save();
    }

    /// Takes `path` off the list, as when it turns out to have gone.
    pub fn remove(&mut self, path: &Path) {
        self.files.retain(|file| file != path);
        self.save();
    }

    fn save(&self) {
        let Some(store) = &self.store else { return };
        let files = self.files.iter().map(|file| toml::Value::String(file.to_string_lossy().into_owned())).collect();
        let table = toml::Table::from_iter([("files".to_string(), toml::Value::Array(files))]);
        let written = store.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::write(store, table.to_string()));
        if let Err(error) = written {
            log::warn!("can't remember recent files in {}: {error}", store.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_is_newest_first_without_repeats_and_survives_a_restart() {
        let folder = std::env::temp_dir().join(format!("omavec-recent-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let file = |name: &str| {
            let path = folder.join(name);
            std::fs::write(&path, "").unwrap();
            std::fs::canonicalize(path).unwrap()
        };
        let (a, b, c) = (file("a.omavecz"), file("b.omavecz"), file("c.omavecz"));
        let store = folder.join("config/recent.toml");
        let mut recent = Recent::load(store.clone());
        assert!(recent.files().is_empty());
        recent.add(&a);
        recent.add(&b);
        recent.add(&a);
        assert_eq!(recent.files(), [a.clone(), b.clone()]);
        recent.add(&c);
        // What has gone since is dropped on the way back in.
        std::fs::remove_file(&b).unwrap();
        assert_eq!(Recent::load(store.clone()).files(), [c.clone(), a.clone()]);
        recent.remove(&c);
        assert_eq!(Recent::load(store.clone()).files(), std::slice::from_ref(&a));
        // Only so many are kept.
        for index in 0..15 {
            recent.add(&file(&format!("{index}.omavecz")));
        }
        assert_eq!(Recent::load(store).files().len(), MOST);
        // A list kept nowhere still works.
        let mut nowhere = Recent::default();
        nowhere.add(&a);
        assert_eq!(nowhere.files(), [a]);
        let _ = std::fs::remove_dir_all(folder);
    }
}
