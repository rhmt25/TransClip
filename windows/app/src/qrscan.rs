//! Webcam QR scanner for the pairing panel. A worker thread reads frames from the first camera,
//! sends a square preview to the window and tells it when a TransClip pairing code was seen.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use nokhwa::pixel_format::RgbFormat;
use nokhwa::utils::{CameraIndex, RequestedFormat, RequestedFormatType};
use nokhwa::Camera;

/// Largest side of the square that is previewed and decoded.
const MAX_SIDE: usize = 480;
/// Pairing codes start like this; anything else a camera sees is ignored.
const PREFIX: &str = "transclip://pair";

/// What the worker reports.
pub enum ScanEvent {
    /// A preview frame: `side` x `side` RGBA pixels.
    Frame { side: usize, rgba: Vec<u8> },
    /// A pairing code was read.
    Found(String),
    /// The camera could not be used.
    Failed(String),
}

/// Handle to a running scan. Dropping it stops the camera.
pub struct Scanner {
    rx: mpsc::Receiver<ScanEvent>,
    stop: Arc<AtomicBool>,
}

impl Scanner {
    /// Start scanning; `wake` is called whenever there is something new to draw.
    pub fn start(wake: impl Fn() + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        std::thread::spawn(move || {
            if let Err(message) = run(&tx, &flag, &wake) {
                let _ = tx.send(ScanEvent::Failed(message));
                wake();
            }
        });
        Self { rx, stop }
    }

    /// Everything that arrived since the last call; only the newest frame is kept.
    pub fn poll(&self) -> Vec<ScanEvent> {
        let mut out: Vec<ScanEvent> = Vec::new();
        while let Ok(ev) = self.rx.try_recv() {
            if matches!(ev, ScanEvent::Frame { .. }) {
                out.retain(|e| !matches!(e, ScanEvent::Frame { .. }));
            }
            out.push(ev);
        }
        out
    }
}

impl Drop for Scanner {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

const NO_CAMERA: &str = "Kamera tidak ditemukan atau aksesnya ditolak. Nyalakan \"Izinkan aplikasi desktop mengakses kamera\" di Pengaturan Windows, Privasi, Kamera.";

fn run(tx: &mpsc::Sender<ScanEvent>, stop: &AtomicBool, wake: &dyn Fn()) -> Result<(), String> {
    let format = RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestFrameRate);
    let mut camera =
        Camera::new(CameraIndex::Index(0), format).map_err(|_| NO_CAMERA.to_owned())?;
    camera.open_stream().map_err(|_| NO_CAMERA.to_owned())?;
    let mut frame_no = 0u32;
    while !stop.load(Ordering::Relaxed) {
        let Ok(buffer) = camera.frame() else {
            std::thread::sleep(Duration::from_millis(30));
            continue;
        };
        let res = buffer.resolution();
        let (w, h) = (res.width() as usize, res.height() as usize);
        let mut rgb = vec![0u8; w * h * 3];
        if buffer
            .decode_image_to_buffer::<RgbFormat>(&mut rgb)
            .is_err()
        {
            continue;
        }
        let Some((side, rgba, gray)) = square_preview(&rgb, w, h) else {
            continue;
        };
        frame_no += 1;
        // Decoding is the expensive part; every third frame is plenty.
        if frame_no.is_multiple_of(3) {
            if let Some(code) = find_code(side, &gray) {
                let _ = tx.send(ScanEvent::Found(code));
                wake();
                return Ok(());
            }
        }
        if tx.send(ScanEvent::Frame { side, rgba }).is_err() {
            return Ok(());
        }
        wake();
    }
    Ok(())
}

/// Centre square of an RGB frame, reduced to at most [`MAX_SIDE`]: (side, RGBA preview, grey).
pub fn square_preview(rgb: &[u8], w: usize, h: usize) -> Option<(usize, Vec<u8>, Vec<u8>)> {
    if w == 0 || h == 0 || rgb.len() < w * h * 3 {
        return None;
    }
    let crop = w.min(h);
    let step = crop.div_ceil(MAX_SIDE).max(1);
    let side = crop / step;
    if side == 0 {
        return None;
    }
    let (x0, y0) = ((w - crop) / 2, (h - crop) / 2);
    let mut rgba = Vec::with_capacity(side * side * 4);
    let mut gray = Vec::with_capacity(side * side);
    for y in 0..side {
        for x in 0..side {
            let i = ((y0 + y * step) * w + (x0 + x * step)) * 3;
            let (r, g, b) = (rgb[i], rgb[i + 1], rgb[i + 2]);
            rgba.extend_from_slice(&[r, g, b, 255]);
            let lum = (u32::from(r) * 30 + u32::from(g) * 59 + u32::from(b) * 11) / 100;
            gray.push(u8::try_from(lum).unwrap_or(255));
        }
    }
    Some((side, rgba, gray))
}

/// A TransClip pairing code in a grey square image, if there is one.
pub fn find_code(side: usize, gray: &[u8]) -> Option<String> {
    let mut image =
        rqrr::PreparedImage::prepare_from_greyscale(side, side, |x, y| gray[y * side + x]);
    image
        .detect_grids()
        .into_iter()
        .filter_map(|grid| grid.decode().ok())
        .map(|(_, content)| content)
        .find(|content| content.starts_with(PREFIX))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Render `text` as a QR code into a grey square image with a quiet zone.
    fn qr_gray(text: &str, scale: usize) -> (usize, Vec<u8>) {
        let code = qrcode::QrCode::new(text.as_bytes()).unwrap();
        let modules = code.width();
        let quiet = 4;
        let side = (modules + 2 * quiet) * scale;
        let colors = code.to_colors();
        let mut px = vec![255u8; side * side];
        for y in 0..modules {
            for x in 0..modules {
                if colors[y * modules + x] == qrcode::Color::Dark {
                    for dy in 0..scale {
                        for dx in 0..scale {
                            px[((y + quiet) * scale + dy) * side + (x + quiet) * scale + dx] = 0;
                        }
                    }
                }
            }
        }
        (side, px)
    }

    #[test]
    fn reads_a_pairing_code_from_an_image() {
        let uri = "transclip://pair?v=1&k=Fw6I9SSL3av6jJ0FNrxQ5xZvVetFzvh58S4Gf_IGL4g&g=TransClip&d=e1bd36038e148152f9612881ba3da0b7&a=192.168.1.16%3A47820&x=1794974934";
        let (side, gray) = qr_gray(uri, 4);
        assert_eq!(find_code(side, &gray).as_deref(), Some(uri));
    }

    #[test]
    fn other_qr_codes_are_ignored() {
        let (side, gray) = qr_gray("https://example.com/not-ours", 4);
        assert_eq!(find_code(side, &gray), None);
    }

    #[test]
    fn preview_is_a_centred_square_no_larger_than_the_limit() {
        // A 1280x720 frame: the square is 720 wide, reduced by 2 to 360.
        let rgb = vec![128u8; 1280 * 720 * 3];
        let (side, rgba, gray) = square_preview(&rgb, 1280, 720).unwrap();
        assert!(side <= MAX_SIDE && side > 0);
        assert_eq!(rgba.len(), side * side * 4);
        assert_eq!(gray.len(), side * side);
        assert!(square_preview(&rgb[..10], 1280, 720).is_none());
    }
}
