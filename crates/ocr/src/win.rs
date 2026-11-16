//! CivilOps PDF: Windows' own text recognition (Windows.Media.Ocr) as the `windows` engine. It is
//! part of Windows 10 and 11 and reads every language whose text recognition is installed with
//! the language (Settings ▸ Time & language ▸ Language), German with its umlauts included, so
//! there are no models to ship.

use std::sync::{Arc, Mutex};

use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;
use windows::core::HSTRING;

use crate::{Line, OcrError, OcrImage, RecognizeOptions, Recognizer, Word};

/// Tiles overlap by this much (pixels) when an image is larger than Windows reads at once, so a
/// word cut by one tile's edge is whole in the next.
const OVERLAP: u32 = 256;

/// The languages Windows reads: their tags (`de-DE`) and names in the user's language.
pub(crate) fn languages() -> Vec<(String, String)> {
    let Ok(list) = OcrEngine::AvailableRecognizerLanguages() else { return Vec::new() };
    list.into_iter().filter_map(|l| Some((l.LanguageTag().ok()?.to_string(), l.DisplayName().ok()?.to_string()))).collect()
}

/// Windows' own text recognition as a [`Recognizer`] (`"windows"`). It reads the language a job
/// names: a tag from [`crate::windows_languages`] (`de-DE`), or a language such as `de` for the
/// installed `de-DE`. One Windows engine per language is made on first use and kept.
#[derive(Default)]
pub struct WindowsRecognizer {
    engines: Mutex<Vec<(String, Arc<Engine>)>>,
}

impl WindowsRecognizer {
    pub fn new() -> WindowsRecognizer {
        WindowsRecognizer::default()
    }

    /// The engine for `language` (made once, then shared).
    fn engine(&self, language: &str) -> Result<Arc<Engine>, OcrError> {
        let tag = crate::resolve_in(crate::windows_languages(), language).ok_or_else(|| OcrError::Language(language.into()))?;
        let mut engines = self.engines.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((_, engine)) = engines.iter().find(|(t, _)| *t == tag) {
            return Ok(engine.clone());
        }
        let engine = Arc::new(Engine::new(&tag)?);
        engines.push((tag, engine.clone()));
        Ok(engine)
    }
}

impl Recognizer for WindowsRecognizer {
    fn id(&self) -> &'static str {
        "windows"
    }

    fn available(&self) -> bool {
        crate::windows_ocr()
    }

    fn languages(&self) -> Vec<(String, String)> {
        crate::windows_languages().to_vec()
    }

    fn recognize(&self, image: &OcrImage, options: &RecognizeOptions) -> Result<Vec<Line>, OcrError> {
        self.engine(&options.language)?.recognize(&gray(image), image.width(), image.height())
    }
}

/// The image as grey, one byte per pixel (Rec. 601 weights).
fn gray(image: &OcrImage) -> Vec<u8> {
    // At most 255 · 1000 / 1000, so it fits a byte.
    let luma = |p: &[u8; 4]| ((u32::from(p[0]) * 299 + u32::from(p[1]) * 587 + u32::from(p[2]) * 114) / 1000) as u8;
    image.rgba().as_chunks::<4>().0.iter().map(luma).collect()
}

/// A recogniser for one language.
struct Engine {
    engine: OcrEngine,
    /// The longest side Windows reads at once.
    max: u32,
}

impl Engine {
    fn new(tag: &str) -> Result<Engine, OcrError> {
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
    fn recognize(&self, gray: &[u8], width: u32, height: u32) -> Result<Vec<Line>, OcrError> {
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
                // Windows reports no confidence for a word.
                words.push(Word::new(text, [r.X + dx, r.Y + dy, r.X + r.Width + dx, r.Y + r.Height + dy], "windows"));
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

    #[test]
    fn images_turn_grey() {
        let image = OcrImage::new(3, 1, vec![255, 255, 255, 255, 0, 0, 0, 255, 255, 0, 0, 255]).unwrap();
        assert_eq!(gray(&image), [255, 0, 76]);
    }

    /// Windows' own text recognition, when Windows has it (Windows 10 and 11 with a language
    /// that includes it).
    #[test]
    fn windows_reads_rendered_text() {
        if !crate::windows_ocr() {
            eprintln!("skipped: no Windows text recognition");
            return;
        }
        let (w, h, px) = crate::tests::test_image::hello();
        let image = OcrImage::new(w, h, px).unwrap();
        let language = crate::resolve_in(crate::windows_languages(), "en").unwrap_or_else(crate::default_language);
        let r = WindowsRecognizer::new();
        assert!(r.available() && r.languages().iter().any(|(c, _)| *c == language));
        let lines = r.recognize(&image, &RecognizeOptions { language }).unwrap();
        let text: Vec<String> = lines.iter().map(Line::text).collect();
        assert!(text.iter().any(|t| t.contains("HELL") && t.contains("WOR")), "{text:?}");
        let word = lines.iter().flat_map(|l| &l.words).find(|w| w.text.contains("HELL")).unwrap();
        assert!(word.rect[0] >= 10.0 && word.rect[0] < 60.0, "{word:?}");
        assert_eq!((word.source.as_str(), word.confidence), ("windows", None));
        assert!(matches!(r.recognize(&image, &RecognizeOptions { language: "xx-YY".into() }), Err(OcrError::Language(_))));
    }
}
