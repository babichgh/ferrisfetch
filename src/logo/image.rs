//! Fastfetch-style Kitty pixel backend: RGBA, base64, 4096-byte chunks.
//! No subprocesses, filesystem access by the terminal, or symbolic approximation.

use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};

use base64::{Engine, engine::general_purpose::STANDARD};
use image::{ImageFormat, ImageReader, Limits, RgbaImage};

use crate::render::{Entry, Layout, color::Style};

#[cfg(unix)]
mod probe;

/// Negotiate the protocol, not a terminal brand. Only called for usable images.
pub fn protocol_supported() -> bool {
    #[cfg(unix)]
    {
        probe::supported().unwrap_or(false)
    }
    #[cfg(not(unix))]
    false
}

const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_DECODE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PIXELS: u64 = 16 * 1024 * 1024;
const MAX_DIMENSION: u32 = 8192;
const MAX_OUTPUT_PIXELS: u64 = 4 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub path: PathBuf,
    pub width: u16,
}

impl Source {
    pub fn from_name(name: &str) -> Option<Self> {
        image_extension(Path::new(name)).then(|| Self {
            path: name.into(),
            width: 28,
        })
    }
}

fn image_extension(path: &Path) -> bool {
    path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
        ["png", "jpg", "jpeg"]
            .iter()
            .any(|ext| s.eq_ignore_ascii_case(ext))
    })
}

#[derive(Clone, Copy, Debug)]
pub struct Terminal {
    pub columns: u16,
    pub rows: u16,
    pub pixel_width: u16,
    pub pixel_height: u16,
}

impl Terminal {
    pub fn detect() -> Option<Self> {
        if !std::io::stdout().is_terminal() {
            return None;
        }
        #[cfg(unix)]
        {
            // Query stdout itself, not an unrelated stdin or controlling TTY.
            let size = rustix::termios::tcgetwinsize(std::io::stdout()).ok()?;
            let terminal = Self {
                columns: size.ws_col,
                rows: size.ws_row,
                pixel_width: size.ws_xpixel,
                pixel_height: size.ws_ypixel,
            };
            terminal.valid().then_some(terminal)
        }
        #[cfg(not(unix))]
        None
    }

    fn valid(self) -> bool {
        self.columns > 1
            && self.rows > 1
            && self.pixel_width >= self.columns
            && self.pixel_height >= self.rows
    }
}

pub struct KittyImage {
    pixels: RgbaImage,
    columns: usize,
    rows: usize,
}

impl KittyImage {
    pub fn load(
        source: &Source,
        terminal: Terminal,
        layout: &Layout,
        entries: &[Entry],
    ) -> Option<Self> {
        // Geometry and decoding are independent of terminal I/O for fixture tests.
        let image = decode(&source.path)?;
        let (width, height, columns, rows) = dimensions(
            image.width(),
            image.height(),
            source.width,
            terminal,
            layout,
            entries,
        )?;
        let pixels = resize_rgba(image, width, height);
        Some(Self {
            pixels,
            columns,
            rows,
        })
    }

    pub fn render(&self, layout: &Layout, entries: &[Entry], style: &Style) -> String {
        // Make room BEFORE placing pixels, including when invoked near the bottom
        // of the screen. A saved cursor would become stale when this scrolls.
        let mut out = format!("\r{}\x1b[{}A\r", "\n".repeat(self.rows), self.rows);
        let encoded = STANDARD.encode(self.pixels.as_raw());
        let chunks = encoded.as_bytes().chunks(4096);
        let count = chunks.len();
        for (index, chunk) in chunks.enumerate() {
            out.push_str("\x1b_G");
            if index == 0 {
                out.push_str(&format!(
                    "a=T,f=32,t=d,s={},v={},C=1,q=2,",
                    self.pixels.width(),
                    self.pixels.height(),
                ));
            }
            out.push_str(if index + 1 == count { "m=0;" } else { "m=1;" });
            out.push_str(std::str::from_utf8(chunk).expect("base64 is ASCII"));
            out.push_str("\x1b\\");
        }
        // C=1 leaves the cursor at the placement origin. Text and blank lines
        // advance it below BOTH columns, even when the image is taller.
        out.push_str(&layout.render_reserved(self.columns, self.rows, entries, style));
        out
    }
}

fn decode(path: &Path) -> Option<RgbaImage> {
    if !image_extension(path) {
        return None;
    }
    #[cfg(unix)]
    let file: std::fs::File = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::CLOEXEC | rustix::fs::OFlags::NONBLOCK,
        rustix::fs::Mode::empty(),
    )
    .ok()?
    .into();
    #[cfg(not(unix))]
    let file = std::fs::File::open(path).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return None;
    }
    let mut reader = ImageReader::new(std::io::Cursor::new(&bytes))
        .with_guessed_format()
        .ok()?;
    if !matches!(reader.format(), Some(ImageFormat::Png | ImageFormat::Jpeg)) {
        return None;
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    reader.limits(limits);
    let decoder = reader.into_decoder().ok()?;
    let (width, height) = image::ImageDecoder::dimensions(&decoder);
    if !allowed_dimensions(width, height) {
        return None;
    }
    Some(image::DynamicImage::from_decoder(decoder).ok()?.to_rgba8())
}

fn allowed_dimensions(width: u32, height: u32) -> bool {
    width > 0
        && height > 0
        && width <= MAX_DIMENSION
        && height <= MAX_DIMENSION
        && u64::from(width) * u64::from(height) <= MAX_PIXELS
}

fn dimensions(
    width: u32,
    height: u32,
    requested: u16,
    terminal: Terminal,
    layout: &Layout,
    entries: &[Entry],
) -> Option<(u32, u32, usize, usize)> {
    if !terminal.valid() || width == 0 || height == 0 || requested == 0 {
        return None;
    }
    // Keep a spare column to prevent autowrap at the terminal's right edge.
    let available =
        usize::from(terminal.columns).checked_sub(1 + layout.gap + layout.text_width(entries))?;
    let columns = usize::from(requested).min(available);
    if columns == 0 {
        return None;
    }
    let cell_width = f64::from(terminal.pixel_width) / f64::from(terminal.columns);
    let cell_height = f64::from(terminal.pixel_height) / f64::from(terminal.rows);
    let scale = (columns as f64 * cell_width / f64::from(width))
        .min(f64::from(terminal.rows - 1) * cell_height / f64::from(height))
        .min((MAX_OUTPUT_PIXELS as f64 / (f64::from(width) * f64::from(height))).sqrt());
    let pixel_width = (f64::from(width) * scale).floor().max(1.0) as u32;
    let pixel_height = (f64::from(height) * scale).floor().max(1.0) as u32;
    Some((
        pixel_width,
        pixel_height,
        (f64::from(pixel_width) / cell_width).ceil() as usize,
        (f64::from(pixel_height) / cell_height).ceil() as usize,
    ))
}

fn resize_rgba(mut pixels: RgbaImage, width: u32, height: u32) -> RgbaImage {
    if pixels.dimensions() == (width, height) {
        return pixels;
    }
    // image's resampler expects premultiplied alpha. Avoid dark/colored fringes
    // around transparent PNG edges, then return straight RGBA for Kitty.
    for pixel in pixels.pixels_mut() {
        for channel in 0..3 {
            pixel[channel] = ((u16::from(pixel[channel]) * u16::from(pixel[3]) + 127) / 255) as u8;
        }
    }
    let mut resized = image::imageops::resize(
        &pixels,
        width,
        height,
        image::imageops::FilterType::Triangle,
    );
    for pixel in resized.pixels_mut() {
        if pixel[3] > 0 {
            for channel in 0..3 {
                pixel[channel] =
                    ((u16::from(pixel[channel]) * 255 / u16::from(pixel[3])).min(255)) as u8;
            }
        }
    }
    resized
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terminal() -> Terminal {
        Terminal {
            columns: 120,
            rows: 40,
            pixel_width: 960,
            pixel_height: 640,
        }
    }

    #[test]
    fn recognizes_paths_without_breaking_names() {
        for name in ["./ferris.png", "/a b/ferris.jpg", "a b.JPEG"] {
            assert_eq!(Source::from_name(name).unwrap().path, PathBuf::from(name));
        }
        for name in ["arch", "off", "", "ferris.svg"] {
            assert!(Source::from_name(name).is_none());
        }
    }

    #[test]
    fn geometry_preserves_pixel_aspect_and_reserves_cells() {
        let layout = Layout::default();
        // Cells are 8x16, so a square is twice as many columns as rows.
        assert_eq!(
            dimensions(100, 100, 28, terminal(), &layout, &[]),
            Some((224, 224, 28, 14))
        );
        assert_eq!(
            dimensions(200, 100, 28, terminal(), &layout, &[]),
            Some((224, 112, 28, 7))
        );
        assert_eq!(
            dimensions(100, 1000, 28, terminal(), &layout, &[]),
            Some((62, 624, 8, 39))
        );
        let narrow = Terminal {
            columns: 10,
            pixel_width: 80,
            ..terminal()
        };
        let entries = [Entry {
            key: "OS".into(),
            value: "Linux".into(),
        }];
        assert!(dimensions(100, 100, 28, narrow, &layout, &entries).is_none());
        let smaller = Terminal {
            columns: 30,
            pixel_width: 240,
            ..terminal()
        };
        assert_eq!(
            dimensions(100, 100, 28, smaller, &layout, &entries),
            Some((128, 128, 16, 8))
        );
        assert!(dimensions(0, 100, 28, terminal(), &layout, &[]).is_none());
        assert!(
            dimensions(
                100,
                100,
                28,
                Terminal {
                    pixel_width: 0,
                    ..terminal()
                },
                &layout,
                &[]
            )
            .is_none()
        );
    }

    #[test]
    fn protocol_chunks_round_trip_rgba_and_cursor_reservation() {
        let pixels = RgbaImage::from_pixel(64, 64, image::Rgba([240, 80, 20, 128]));
        let image = KittyImage {
            pixels: pixels.clone(),
            columns: 8,
            rows: 4,
        };
        let style = Style {
            colors: false,
            ..Default::default()
        };
        for count in [0, 1, 7] {
            let entries = vec![
                Entry {
                    key: "OS".into(),
                    value: "Linux".into()
                };
                count
            ];
            let output = image.render(&Layout::default(), &entries, &style);
            assert!(
                output.starts_with("\r\n\n\n\n\x1b[4A\r\x1b_Ga=T,f=32,t=d,s=64,v=64,C=1,q=2,m=1;")
            );
            let mut encoded = String::new();
            let mut remaining = output.split_once("\x1b_G").unwrap().1;
            loop {
                let (command, after) = remaining.split_once("\x1b\\").unwrap();
                let (header, payload) = command.split_once(';').unwrap();
                assert!(payload.len() <= 4096);
                assert_eq!(payload.len() % 4, 0);
                encoded.push_str(payload);
                if header.ends_with("m=0") {
                    assert_eq!(after.lines().count(), count.max(4));
                    assert!(after.ends_with('\n'));
                    for line in after.lines().filter(|line| line.contains("OS")) {
                        assert!(line.starts_with(&" ".repeat(11)));
                    }
                    break;
                }
                assert!(header.ends_with("m=1"));
                remaining = after.strip_prefix("\x1b_G").unwrap();
            }
            assert_eq!(STANDARD.decode(encoded).unwrap(), *pixels.as_raw());
        }
    }

    #[test]
    fn transparent_edges_are_not_blended_with_hidden_rgb() {
        let pixels = RgbaImage::from_fn(2, 1, |x, _| {
            if x == 0 {
                image::Rgba([255, 0, 0, 255])
            } else {
                image::Rgba([0, 255, 0, 0])
            }
        });
        let resized = resize_rgba(pixels, 4, 2);
        for pixel in resized.pixels().filter(|pixel| pixel[3] > 0) {
            assert_eq!(pixel[1], 0);
            assert!(pixel[0] >= 254);
        }
        assert!(resized.pixels().any(|pixel| pixel[3] == 0));
    }

    #[test]
    fn decoded_dimensions_are_bounded_before_allocating_pixels() {
        assert!(allowed_dimensions(4096, 4096));
        assert!(allowed_dimensions(8192, 2048));
        for (width, height) in [
            (0, 1),
            (1, 0),
            (8193, 1),
            (1, 8193),
            (4097, 4097),
            (u32::MAX, u32::MAX),
        ] {
            assert!(!allowed_dimensions(width, height));
        }
    }

    #[test]
    fn cursor_finishes_below_image_and_text_even_when_output_scrolls() {
        let style = Style {
            colors: false,
            ..Default::default()
        };
        // Minimal interpreter for this backend's output (APC, CUU, CR, LF,
        // ASCII), with tty ONLCR. This is NOT a visual Kitty verification.
        for image_rows in [1, 7, 39] {
            for text_rows in [0, 1, 15, 45] {
                for start_row in [0, 30, 39] {
                    let image = KittyImage {
                        pixels: RgbaImage::new(8, image_rows * 16),
                        columns: 1,
                        rows: image_rows as usize,
                    };
                    let entries = vec![
                        Entry {
                            key: "K".into(),
                            value: "V".into()
                        };
                        text_rows
                    ];
                    let output = image.render(&Layout::default(), &entries, &style);
                    let mut remaining = output.as_str();
                    let mut row = start_row;
                    let mut column = 0;
                    let mut image_bottom = None;
                    let mut text_bottom = None;
                    while !remaining.is_empty() {
                        if let Some(apc) = remaining.strip_prefix("\x1b_G") {
                            let (command, after) = apc.split_once("\x1b\\").unwrap();
                            if command.split_once(';').unwrap().0.ends_with("m=0") {
                                image_bottom = Some(row + image_rows as i32 - 1);
                                assert!(image_bottom.unwrap() < 40, "image clipped before text");
                            }
                            remaining = after;
                        } else if let Some(csi) = remaining.strip_prefix("\x1b[") {
                            let (count, after) = csi.split_once('A').unwrap();
                            row -= count.parse::<i32>().unwrap();
                            assert!(row >= 0);
                            remaining = after;
                        } else {
                            let byte = remaining.as_bytes()[0];
                            remaining = &remaining[1..];
                            match byte {
                                b'\r' => column = 0,
                                b'\n' => {
                                    column = 0;
                                    if row == 39 {
                                        image_bottom = image_bottom.map(|r| r - 1);
                                        text_bottom = text_bottom.map(|r| r - 1);
                                    } else {
                                        row += 1;
                                    }
                                }
                                _ => {
                                    column += 1;
                                    if byte != b' ' {
                                        text_bottom = Some(row);
                                    }
                                }
                            }
                        }
                    }
                    assert_eq!(column, 0);
                    assert!(row > image_bottom.unwrap());
                    assert!(text_bottom.is_none_or(|bottom| row > bottom));
                }
            }
        }
    }
}
