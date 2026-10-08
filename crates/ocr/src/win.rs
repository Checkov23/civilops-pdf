//! CivilOps PDF: Windows' own text recognition (Windows.Media.Ocr). It is part of Windows 10 and
//! 11 and reads every language whose text recognition is installed with the language (Settings ▸
//! Time & language ▸ Language), German with its umlauts included, so there are no models to ship.

use std::sync::Mutex;

use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;
use windows::core::HSTRING;

use crate::{Line, OcrError, Word};

/// Tiles overlap by this much (pixels) when an image is larger than Windows reads at once, so a
/// word cut by one tile's edge is whole in the next.
const OVERLAP: u32 = 256;

/// The languages Windows reads: their tags (`de-DE`) and names in the user's language.
pub fn languages() -> Vec<(String, String)> {
    let Ok(list) = OcrEngine::AvailableRecognizerLanguages() else { return Vec::new() };
    list.into_iter().filter_map(|l| Some((l.LanguageTag().ok()?.to_string(), l.DisplayName().ok()?.to_string()))).collect()
}

/// A recogniser for one language.
pub struct Engine {
    engine: OcrEngine,
    /// The longest side Windows reads at once.
    max: u32,
}

impl Engine {
    pub fn new(tag: &str) -> Result<Engine, OcrError> {
        let err = |e: windows::core::Error| OcrError::Load(format!("Windows text recognition ({tag})"), e.to_string());
        let language = Language::CreateLanguage(&HSTRING::from(tag)).map_err(err)?;
        if !OcrEngine::IsLanguageSupported(&language).map_err(err)? {
            return Err(OcrError::Language(tag.into()));
        }
        let engine = OcrEngine::TryCreateFromLanguage(&language).map_err(err)?;
        let max = OcrEngine::MaxImageDimension().unwrap_or(4096).clamp(1024, 16_384);
        Ok(Engine { engine, max })
    }

    /// Read a grey image (one byte per pixel), in tiles when it is larger than Windows reads at
    /// once.
    pub fn recognize(&self, gray: &[u8], width: u32, height: u32) -> Result<Vec<Line>, OcrError> {
        let (xs, ys) = (tiles(width, self.max), tiles(height, self.max));
        let mut out = Vec::new();
        for (xi, &(x0, x1)) in xs.iter().enumerate() {
            for (yi, &(y0, y1)) in ys.iter().enumerate() {
                // A word belongs to the tile whose middle part holds its centre.
                let (kx0, kx1) = core(&xs, xi);
                let (ky0, ky1) = core(&ys, yi);
                let keep = |r: &[f32; 4]| {
                    let (cx, cy) = ((r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0);
                    cx >= kx0 && cx < kx1 && cy >= ky0 && cy < ky1
                };
                for line in self.tile(gray, width, [x0, y0, x1, y1])? {
                    let words: Vec<Word> = line.words.into_iter().filter(|w| keep(&w.rect)).collect();
                    if !words.is_empty() {
                        out.push(Line { words });
                    }
                }
            }
        }
        Ok(out)
    }

    /// Read the part `[x0, y0, x1, y1)` of the image; word boxes are in whole-image pixels.
    fn tile(&self, gray: &[u8], width: u32, [x0, y0, x1, y1]: [u32; 4]) -> Result<Vec<Line>, OcrError> {
        let err = |e: windows::core::Error| OcrError::Recognize(e.to_string());
        let (w, h) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
        let mut part = Vec::with_capacity(w as usize * h as usize);
        for y in y0..y1 {
            let start = y as usize * width as usize + x0 as usize;
            let row = gray.get(start..start + w as usize).ok_or(OcrError::EmptyImage)?;
            part.extend_from_slice(row);
        }
        let writer = DataWriter::new().map_err(err)?;
        writer.WriteBytes(&part).map_err(err)?;
        let buffer = writer.DetachBuffer().map_err(err)?;
        let to_i32 = |v: u32| i32::try_from(v).map_err(|_| OcrError::EmptyImage);
        let bitmap = SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Gray8, to_i32(w)?, to_i32(h)?).map_err(err)?;
        // Two recognitions at once make one of them find nothing, so they take turns.
        static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
        let turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
        let result = self.engine.RecognizeAsync(&bitmap).map_err(err)?.join().map_err(err)?;
        drop(turn);
        let (dx, dy) = (x0 as f32, y0 as f32);
        let mut lines = Vec::new();
        for line in result.Lines().map_err(err)? {
            let mut words = Vec::new();
            for word in line.Words().map_err(err)? {
                let text = word.Text().map_err(err)?.to_string();
                let r = word.BoundingRect().map_err(err)?;
                if text.trim().is_empty() || !(r.Width > 0.0 && r.Height > 0.0) {
                    continue;
                }
                words.push(Word { text, rect: [r.X + dx, r.Y + dy, r.X + r.Width + dx, r.Y + r.Height + dy] });
            }
            if !words.is_empty() {
                lines.push(Line { words });
            }
        }
        Ok(lines)
    }
}

/// Split `len` pixels into evenly spaced ranges of `max` that overlap by at least [`OVERLAP`].
fn tiles(len: u32, max: u32) -> Vec<(u32, u32)> {
    if len <= max || max <= OVERLAP {
        return vec![(0, len)];
    }
    let n = (len - OVERLAP).div_ceil(max - OVERLAP).max(2);
    let span = u64::from(len - max);
    (0..n)
        .map(|i| {
            // At most `span`, so it fits.
            let start = u32::try_from(span * u64::from(i) / u64::from(n - 1)).unwrap_or(len - max);
            (start, start + max)
        })
        .collect()
}

/// The part of tile `i` that no neighbour reads better: up to the middle of each overlap.
fn core(tiles: &[(u32, u32)], i: usize) -> (f32, f32) {
    let start = match i.checked_sub(1).and_then(|p| tiles.get(p)) {
        Some(prev) => (prev.1 as f32 + tiles.get(i).map_or(0.0, |t| t.0 as f32)) / 2.0,
        None => f32::NEG_INFINITY,
    };
    let end = match (tiles.get(i), tiles.get(i + 1)) {
        (Some(t), Some(next)) => (t.1 as f32 + next.0 as f32) / 2.0,
        _ => f32::INFINITY,
    };
    (start, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_images_are_read_in_overlapping_tiles() {
        assert_eq!(tiles(800, 1000), [(0, 800)]);
        let t = tiles(2500, 1000);
        assert_eq!(t, [(0, 1000), (500, 1500), (1000, 2000), (1500, 2500)]);
        for w in t.windows(2) {
            assert!(w[0].1 - w[1].0 >= OVERLAP, "{t:?} overlap");
        }
        assert!(tiles(30_000, 10_000).iter().all(|t| t.1 - t.0 == 10_000 && t.1 <= 30_000));
        // Every pixel belongs to exactly one tile's middle part.
        for x in [0.0, 700.0, 871.9, 872.0, 1700.0, 2499.0] {
            assert_eq!((0..t.len()).filter(|&i| core(&t, i).0 <= x && x < core(&t, i).1).count(), 1, "{x}");
        }
    }
}
