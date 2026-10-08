//! CivilOps PDF: printing on Windows.
//!
//! Windows has no spooler that takes PDF, so each sheet of the print-ready PDF is rendered at
//! 300 dpi (less for very large sheets, see `printcraft_render::effective_scale`) and handed to
//! the printer by .NET's `System.Drawing.Printing` through PowerShell, which every Windows has.
//! No unsafe code: the job runs in a child process without a window. The sheet size picks the
//! printer's paper (A4, A3, …) and orientation; copies, collation, duplex and grayscale are
//! passed through.

use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::Arc;

use printcraft_render::{PageRenderer, RenderConfig, RenderRequest};

use crate::PrintError;
use crate::spool::{Duplex, Job, Printer};

/// `CREATE_NO_WINDOW`: no console window flashes up for the child process.
const NO_WINDOW: u32 = 0x0800_0000;
/// Print resolution (dots per inch).
const DPI: f32 = 300.0;

/// Lists the printers: one line per printer, the default one marked with `*`.
const LIST_SCRIPT: &str = r#"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
Add-Type -AssemblyName System.Drawing
$d = (New-Object System.Drawing.Printing.PrinterSettings).PrinterName
foreach ($p in [System.Drawing.Printing.PrinterSettings]::InstalledPrinters) {
  if ($p -eq $d) { "*$p" } else { " $p" }
}
"#;

/// Prints the sheets listed in `$env:PC_LIST` (one `path|width_pt|height_pt` per line).
const PRINT_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
Add-Type -AssemblyName System.Drawing
$sheets = @(Get-Content -LiteralPath $env:PC_LIST -Encoding UTF8 | Where-Object { $_ -ne '' })
$doc = New-Object System.Drawing.Printing.PrintDocument
if ($env:PC_PRINTER) { $doc.PrinterSettings.PrinterName = $env:PC_PRINTER }
if (-not $doc.PrinterSettings.IsValid) { [Console]::Error.WriteLine("printer not found: $($env:PC_PRINTER)"); exit 2 }
$doc.DocumentName = $env:PC_TITLE
$doc.PrinterSettings.Copies = [int16]$env:PC_COPIES
$doc.PrinterSettings.Collate = ($env:PC_COLLATE -eq '1')
switch ($env:PC_DUPLEX) {
  'long' { $doc.PrinterSettings.Duplex = [System.Drawing.Printing.Duplex]::Vertical }
  'short' { $doc.PrinterSettings.Duplex = [System.Drawing.Printing.Duplex]::Horizontal }
  default { $doc.PrinterSettings.Duplex = [System.Drawing.Printing.Duplex]::Simplex }
}
$doc.DefaultPageSettings.Color = ($env:PC_GRAY -ne '1')
$doc.OriginAtMargins = $false
if ($env:PC_TOFILE) { $doc.PrinterSettings.PrintToFile = $true; $doc.PrinterSettings.PrintFileName = $env:PC_TOFILE }
$script:i = 0
$doc.add_QueryPageSettings({
  param($sender, $e)
  $parts = $sheets[$script:i].Split('|')
  $w = [double]::Parse($parts[1], [Globalization.CultureInfo]::InvariantCulture) / 72 * 100
  $h = [double]::Parse($parts[2], [Globalization.CultureInfo]::InvariantCulture) / 72 * 100
  $e.PageSettings.Landscape = ($w -gt $h)
  $short = [math]::Min($w, $h); $long = [math]::Max($w, $h)
  foreach ($ps in $doc.PrinterSettings.PaperSizes) {
    if ([math]::Abs($ps.Width - $short) -le 6 -and [math]::Abs($ps.Height - $long) -le 6) { $e.PageSettings.PaperSize = $ps; break }
  }
})
$doc.add_PrintPage({
  param($sender, $e)
  $parts = $sheets[$script:i].Split('|')
  $img = [System.Drawing.Image]::FromFile($parts[0])
  try {
    $b = $e.PageBounds
    $e.Graphics.DrawImage($img, [single](-$e.PageSettings.HardMarginX), [single](-$e.PageSettings.HardMarginY), [single]$b.Width, [single]$b.Height)
  } finally { $img.Dispose() }
  $script:i++
  $e.HasMorePages = ($script:i -lt $sheets.Count)
})
$doc.Print()
"#;

/// `-EncodedCommand` takes the script as base64 of UTF-16LE.
fn encoded(script: &str) -> String {
    let bytes: Vec<u8> = script.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    base64(&bytes)
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (k, shift) in [18u32, 12, 6, 0].into_iter().enumerate() {
            if k <= chunk.len() {
                out.push(char::from(T[((n >> shift) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn powershell(script: &str) -> Command {
    let mut c = Command::new("powershell");
    c.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand", &encoded(script)]);
    c.creation_flags(NO_WINDOW);
    c
}

/// The printers Windows knows, the default one marked.
pub(crate) fn printers() -> Vec<Printer> {
    let Ok(out) = powershell(LIST_SCRIPT).output() else { return Vec::new() };
    parse_list(&String::from_utf8_lossy(&out.stdout))
}

pub(crate) fn parse_list(out: &str) -> Vec<Printer> {
    out.lines()
        .filter_map(|l| {
            let l = l.trim_end_matches(['\r', '\n']);
            let (default, name) = match l.strip_prefix('*') {
                Some(n) => (true, n),
                None => (false, l.strip_prefix(' ').unwrap_or(l)),
            };
            (!name.trim().is_empty()).then(|| Printer { name: name.to_string(), default })
        })
        .collect()
}

/// Render every sheet of `pdf` to a PNG in `dir`; returns the list file's lines.
fn render_sheets(pdf: &[u8], dir: &std::path::Path) -> Result<Vec<String>, PrintError> {
    let err = |e: String| PrintError::Spool(e);
    let doc = printcraft_cos::Document::open(Arc::new(pdf.to_vec())).map_err(|e| err(format!("the print-ready PDF can't be read: {e}")))?;
    let sizes: Vec<(f64, f64)> = printcraft_model::pages(&doc)
        .iter()
        .map(|p| {
            let c = p.crop(&doc);
            let (w, h) = ((c[2] - c[0]).abs(), (c[3] - c[1]).abs());
            if p.rotation(&doc).rem_euclid(180) == 90 { (h, w) } else { (w, h) }
        })
        .collect();
    let mut renderer = PageRenderer::new(Arc::new(pdf.to_vec()), RenderConfig::default());
    let mut lines = Vec::with_capacity(sizes.len());
    for (i, (w, h)) in sizes.iter().enumerate() {
        let page = renderer.render(RenderRequest { page: i, scale: DPI / 72.0, ..Default::default() });
        if let Some(e) = page.error {
            return Err(err(format!("sheet {} couldn't be prepared: {e}", i + 1)));
        }
        // Premultiplied RGBA on white: the colour channels are the picture.
        let rgb: Vec<u8> = page.rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
        let path = dir.join(format!("sheet-{:04}.png", i + 1));
        let file = std::fs::File::create(&path).map_err(|e| err(e.to_string()))?;
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), page.width, page.height);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        let mut writer = enc.write_header().map_err(|e| err(e.to_string()))?;
        writer.write_image_data(&rgb).map_err(|e| err(e.to_string()))?;
        writer.finish().map_err(|e| err(e.to_string()))?;
        lines.push(format!("{}|{}|{}", path.display(), w, h));
    }
    if lines.is_empty() {
        return Err(err("there is nothing to print".into()));
    }
    Ok(lines)
}

/// Print a print-ready PDF on a Windows printer.
pub(crate) fn submit(pdf: &[u8], job: &Job) -> Result<String, PrintError> {
    let err = |e: String| PrintError::Spool(e);
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let dir = std::env::temp_dir().join(format!("civilops-pdf-print-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| err(e.to_string()))?;
    let result = (|| {
        let lines = render_sheets(pdf, &dir)?;
        let list = dir.join("sheets.txt");
        std::fs::write(&list, lines.join("\r\n")).map_err(|e| err(e.to_string()))?;
        let out = powershell(PRINT_SCRIPT)
            .env("PC_LIST", &list)
            .env("PC_PRINTER", job.printer.as_deref().unwrap_or(""))
            .env("PC_TITLE", &job.title)
            .env("PC_COPIES", job.copies.clamp(1, 999).to_string())
            .env("PC_COLLATE", if job.collate { "1" } else { "0" })
            .env(
                "PC_DUPLEX",
                match job.duplex {
                    Duplex::Off => "off",
                    Duplex::LongEdge => "long",
                    Duplex::ShortEdge => "short",
                },
            )
            .env("PC_GRAY", if job.grayscale { "1" } else { "0" })
            // Tests print into a file (a printer such as Microsoft Print to PDF) without a dialog.
            .env("PC_TOFILE", std::env::var("CIVILOPS_PRINT_TO_FILE").unwrap_or_default())
            .output()
            .map_err(|e| err(format!("Windows printing isn't available: {e}")))?;
        if out.status.success() {
            Ok(format!("{} sheet(s)", lines.len()))
        } else {
            let e = String::from_utf8_lossy(&out.stderr).trim().to_string();
            Err(err(if e.is_empty() { "the print job was refused".into() } else { e.lines().take(3).collect::<Vec<_>>().join(" ") }))
        }
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn printer_lists_and_base64() {
        let p = parse_list("*Brother HL-L2350DW\r\n Microsoft Print to PDF\r\n \r\n");
        assert_eq!(p.len(), 2);
        assert!(p[0].default && p[0].name == "Brother HL-L2350DW");
        assert!(!p[1].default && p[1].name == "Microsoft Print to PDF");
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
        assert_eq!(encoded("A"), "QQA=");
    }
}
