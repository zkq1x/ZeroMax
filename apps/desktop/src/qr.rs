use anyhow::Result;
use qrcode::{Color, QrCode};
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

/// Render a QR code from `text` into a `SharedPixelBuffer`.
///
/// `module_px` controls how many pixels each QR module (cell) occupies.
/// A 4-module quiet zone is added on every side per the QR spec.
///
/// Returns a `SharedPixelBuffer` (Send + Sync) rather than `Image` directly,
/// because `slint::Image` is `!Send` — the buffer must be converted to `Image`
/// on the Slint event-loop thread (e.g. inside an `upgrade_in_event_loop`).
pub fn render_buffer(text: &str, module_px: u32) -> Result<SharedPixelBuffer<Rgba8Pixel>> {
    let code = QrCode::new(text)?;
    let modules = code.width() as u32;
    let quiet = module_px * 4;
    let size = modules * module_px + quiet * 2;

    let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(size, size);
    let pixels = buffer.make_mut_slice();

    pixels.fill(Rgba8Pixel { r: 255, g: 255, b: 255, a: 255 });

    let colors = code.to_colors();
    for (i, color) in colors.iter().enumerate() {
        if *color != Color::Dark {
            continue;
        }
        let mx = (i as u32) % modules;
        let my = (i as u32) / modules;
        for dy in 0..module_px {
            for dx in 0..module_px {
                let px = quiet + mx * module_px + dx;
                let py = quiet + my * module_px + dy;
                let idx = (py * size + px) as usize;
                pixels[idx] = Rgba8Pixel { r: 0, g: 0, b: 0, a: 255 };
            }
        }
    }

    Ok(buffer)
}

/// Convenience: render directly to `Image`. Must be called on the Slint thread.
#[allow(dead_code)]
pub fn render(text: &str, module_px: u32) -> Result<Image> {
    Ok(Image::from_rgba8(render_buffer(text, module_px)?))
}
