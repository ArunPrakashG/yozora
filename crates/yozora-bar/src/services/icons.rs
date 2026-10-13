//! Freedesktop icon theme lookup, for tray items and application icons.
//!
//! Each theme is indexed once on first use: a walk over its directories that keeps the best
//! file per icon name (scalable first, then the PNG closest to 32 px). Lookups after that are
//! a hash map hit.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

type Index = HashMap<String, (u32, PathBuf)>;

#[derive(Default)]
struct Icons {
    themes: Option<Vec<String>>,
    indexes: HashMap<String, Index>,
    pixmaps: Option<Index>,
}

static ICONS: OnceLock<Mutex<Icons>> = OnceLock::new();

fn base_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        dirs.push(home.join(".icons"));
    }
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    dirs.extend(data_home.map(|d| d.join("icons")));
    let data = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    dirs.extend(data.split(':').filter(|d| !d.is_empty()).map(|d| PathBuf::from(d).join("icons")));
    dirs
}

/// The user's icon theme, from GTK settings or gsettings.
fn user_theme() -> Option<String> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    for file in ["gtk-4.0/settings.ini", "gtk-3.0/settings.ini"] {
        if let Ok(text) = std::fs::read_to_string(config.join(file)) {
            for line in text.lines() {
                if let Some(v) = line.trim().strip_prefix("gtk-icon-theme-name") {
                    let v = v.trim_start_matches([' ', '=']).trim().trim_matches('"');
                    if !v.is_empty() {
                        return Some(v.to_string());
                    }
                }
            }
        }
    }
    let out = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "icon-theme"])
        .output()
        .ok()?;
    let v = String::from_utf8_lossy(&out.stdout).trim().trim_matches('\'').to_string();
    (!v.is_empty()).then_some(v)
}

fn inherits(theme: &str) -> Vec<String> {
    for dir in base_dirs() {
        if let Ok(text) = std::fs::read_to_string(dir.join(theme).join("index.theme")) {
            for line in text.lines() {
                if let Some(v) = line.strip_prefix("Inherits=") {
                    return v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                }
            }
        }
    }
    Vec::new()
}

/// The theme chain: the user's theme and what it inherits, then hicolor.
fn chain() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut queue: Vec<String> = user_theme().into_iter().collect();
    while let Some(t) = queue.pop() {
        if out.contains(&t) || out.len() > 8 {
            continue;
        }
        let mut parents = inherits(&t);
        parents.reverse();
        out.push(t);
        queue.extend(parents);
    }
    for fallback in ["Adwaita", "hicolor"] {
        if !out.iter().any(|t| t == fallback) {
            out.push(fallback.into());
        }
    }
    out
}

/// Lower is better: scalable, then sizes near 32 px.
fn score(path: &Path) -> Option<u32> {
    let ext = path.extension()?.to_str()?;
    if !matches!(ext, "svg" | "png") {
        return None;
    }
    let s = path.to_string_lossy();
    if ext == "svg" {
        return Some(if s.contains("symbolic") { 50 } else { 0 });
    }
    let size = path
        .ancestors()
        .filter_map(|a| a.file_name()?.to_str())
        .find_map(|c| c.split(['x', '@']).next()?.parse::<u32>().ok())
        .unwrap_or(48);
    Some(1 + size.abs_diff(32) + if size < 22 { 40 } else { 0 })
}

fn index_dir(dir: &Path, index: &mut Index, depth: u32) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_dir() || (ft.is_symlink() && path.is_dir()) {
            if depth < 4 {
                index_dir(&path, index, depth + 1);
            }
        } else if let (Some(stem), Some(score)) = (path.file_stem().and_then(|s| s.to_str()), score(&path)) {
            let better = index.get(stem).is_none_or(|(old, _)| score < *old);
            if better {
                index.insert(stem.to_string(), (score, path));
            }
        }
    }
}

fn build(theme: &str) -> Index {
    let mut index = Index::new();
    for base in base_dirs() {
        index_dir(&base.join(theme), &mut index, 0);
    }
    index
}

/// Finds an icon by name, trying `extra` directories (like a tray item's IconThemePath) first.
/// Absolute paths are returned as-is if they exist.
pub fn find(name: &str, extra: &[PathBuf]) -> Option<PathBuf> {
    if name.is_empty() {
        return None;
    }
    if name.starts_with('/') {
        let p = PathBuf::from(name);
        return p.exists().then_some(p);
    }
    for dir in extra {
        let mut index = Index::new();
        index_dir(dir, &mut index, 0);
        if let Some((_, p)) = index.remove(name) {
            return Some(p);
        }
    }
    let lock = ICONS.get_or_init(|| Mutex::new(Icons::default()));
    let mut icons = lock.lock().unwrap_or_else(|e| e.into_inner());
    let themes = icons.themes.get_or_insert_with(chain).clone();
    for theme in themes {
        let index = icons.indexes.entry(theme.clone()).or_insert_with(|| build(&theme));
        if let Some((_, p)) = index.get(name) {
            return Some(p.clone());
        }
    }
    let pixmaps = icons.pixmaps.get_or_insert_with(|| {
        let mut index = Index::new();
        index_dir(Path::new("/usr/share/pixmaps"), &mut index, 2);
        index
    });
    pixmaps.get(name).map(|(_, p)| p.clone())
}

/// The icon for an application, from its desktop entry or by guessing from its id.
pub fn app_icon(id: &str) -> Option<PathBuf> {
    if id.is_empty() {
        return None;
    }
    let lower = id.to_lowercase();
    let data = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    let mut dirs: Vec<PathBuf> = std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(".local/share/applications"))
        .into_iter()
        .collect();
    dirs.extend(data.split(':').map(|d| PathBuf::from(d).join("applications")));
    for dir in &dirs {
        for candidate in [id, lower.as_str()] {
            let file = dir.join(format!("{candidate}.desktop"));
            if let Ok(text) = std::fs::read_to_string(&file) {
                if let Some(icon) = text.lines().find_map(|l| l.strip_prefix("Icon=")) {
                    if let Some(p) = find(icon.trim(), &[]) {
                        return Some(p);
                    }
                }
            }
        }
    }
    find(id, &[]).or_else(|| find(&lower, &[]))
}

/// A copy of a raster image no larger than `max` px on its longer side, or the image itself
/// if it's already small enough or isn't raster.
///
/// GPUI keeps an image decoded at its full size, and uploads it to the GPU at that size, no
/// matter how small it's drawn. Some apps ship only a 512 px icon, which is 1 MB for a 16 px
/// tray icon; album art can be 4 MB. Copies are cached in `~/.cache/yozora/thumbs`, keyed by
/// the path, size and modification time of the original.
pub fn fit(path: &Path, max: u32) -> PathBuf {
    shrink(path, max, &thumbs_dir()).unwrap_or_else(|| path.to_path_buf())
}

fn shrink(path: &Path, max: u32, dir: &Path) -> Option<PathBuf> {
    use std::hash::{Hash, Hasher};
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("svg")) {
        return None;
    }
    let meta = std::fs::metadata(path).ok()?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (path, meta.len(), meta.modified().ok(), max).hash(&mut h);
    let out = dir.join(format!("{:016x}.png", h.finish()));
    if out.exists() {
        return Some(out);
    }
    let reader = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?;
    let (w, h) = reader.into_dimensions().ok()?;
    if w.max(h) <= max {
        return None;
    }
    let image = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?.decode().ok()?;
    let small = image.resize(max, max, image::imageops::FilterType::Triangle);
    std::fs::create_dir_all(dir).ok()?;
    let tmp = out.with_extension("part");
    small.save_with_format(&tmp, image::ImageFormat::Png).ok()?;
    std::fs::rename(&tmp, &out).ok()?;
    Some(out)
}

fn thumbs_dir() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir)
        .join("yozora")
        .join("thumbs")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_shrinks_only_big_raster_images() {
        let dir = std::env::temp_dir().join(format!("yozora-fit-{}", std::process::id()));
        let thumbs = dir.join("thumbs");
        std::fs::create_dir_all(&dir).unwrap();
        let big = dir.join("big.png");
        image::RgbaImage::from_pixel(512, 256, image::Rgba([10, 20, 30, 255])).save(&big).unwrap();
        let small = dir.join("small.png");
        image::RgbaImage::from_pixel(24, 24, image::Rgba([0, 0, 0, 255])).save(&small).unwrap();

        let out = shrink(&big, 32, &thumbs).expect("a copy");
        assert_eq!(image::image_dimensions(&out).unwrap(), (32, 16));
        // The second call reuses the copy.
        assert_eq!(shrink(&big, 32, &thumbs), Some(out));
        assert_eq!(shrink(&small, 32, &thumbs), None);
        assert_eq!(shrink(&dir.join("icon.svg"), 32, &thumbs), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
