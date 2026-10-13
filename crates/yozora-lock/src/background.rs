//! The blurred wallpaper behind the lock screen. GPUI can't blur, so it's done once on the
//! CPU at a small size and cached; drawn stretched over the screen, the small image looks the
//! same as a full-size blur.

use std::{
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};

use image::{DynamicImage, RgbaImage, imageops::FilterType};

/// The prepared image's width. A blur this soft has no detail to lose.
const WIDTH: u32 = 480;
/// At `WIDTH`, about the design's 34 px blur on a 1920 px screen.
const SIGMA: f32 = 8.0;
/// The design's `saturate(150%)`, a little gentler.
const SATURATION: f32 = 1.4;

fn cache_dir() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir)
        .join("yozora")
        .join("lock")
}

/// The image to draw for `source`: a cached, blurred copy, made now if needed.
pub fn prepare(source: &Path, blur: bool) -> anyhow::Result<PathBuf> {
    let meta = std::fs::metadata(source)?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (source, meta.len(), meta.modified().ok(), blur, WIDTH, SIGMA.to_bits()).hash(&mut hasher);
    let out = cache_dir().join(format!("{:016x}.png", hasher.finish()));
    if out.is_file() {
        return Ok(out);
    }
    let started = std::time::Instant::now();
    let image = image::open(source)?;
    let processed = process(image, blur);
    std::fs::create_dir_all(cache_dir())?;
    let tmp = out.with_extension("tmp");
    processed.save_with_format(&tmp, image::ImageFormat::Png)?;
    std::fs::rename(&tmp, &out)?;
    prune(&out);
    tracing::debug!("prepared {} in {:?}", source.display(), started.elapsed());
    Ok(out)
}

fn process(image: DynamicImage, blur: bool) -> RgbaImage {
    if !blur {
        // Still downscaled a little: a 6K wallpaper costs a lot of memory as a texture.
        return image.resize(2560, 2560, FilterType::Triangle).to_rgba8();
    }
    let small = image.resize(WIDTH, WIDTH * 4, FilterType::Triangle).to_rgba8();
    let mut out = image::imageops::blur(&small, SIGMA);
    for p in out.pixels_mut() {
        let [r, g, b, a] = p.0;
        let gray = 0.2126 * f32::from(r) + 0.7152 * f32::from(g) + 0.0722 * f32::from(b);
        let s = |c: u8| (gray + (f32::from(c) - gray) * SATURATION).clamp(0.0, 255.0) as u8;
        p.0 = [s(r), s(g), s(b), a];
    }
    out
}

/// Keeps the cache to the few most recent images.
fn prune(keep: &Path) {
    let Ok(entries) = std::fs::read_dir(cache_dir()) else { return };
    let mut files: Vec<_> = entries
        .flatten()
        .filter(|e| e.path() != keep)
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    files.sort();
    let excess = files.len().saturating_sub(7);
    for (_, path) in files.into_iter().take(excess) {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blurs_small_and_keeps_aspect() {
        let image = DynamicImage::ImageRgba8(RgbaImage::from_fn(1920, 1080, |x, _| {
            if x < 960 { image::Rgba([255, 0, 0, 255]) } else { image::Rgba([0, 0, 255, 255]) }
        }));
        let out = process(image, true);
        assert_eq!(out.width(), WIDTH);
        assert_eq!(out.height(), 270);
        // The edge between the halves is soft now.
        let mid = out.get_pixel(WIDTH / 2, 135).0;
        assert!(mid[0] > 40 && mid[2] > 40, "{mid:?}");
    }
}
