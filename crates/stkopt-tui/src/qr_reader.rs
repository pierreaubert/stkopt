//! QR code reader using camera capture.
//!
//! Uses nokhwa for cross-platform camera access and rqrr for QR decoding.

use nokhwa::Camera;
use nokhwa::pixel_format::RgbFormat;
use nokhwa::utils::{
    CameraFormat, CameraIndex, FrameFormat, RequestedFormat, RequestedFormatType, Resolution,
};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// Preview dimensions for braille rendering (2 cols per char, 4 rows per char).
/// 80x48 gives us 40 chars wide × 12 chars tall of braille.
pub const PREVIEW_WIDTH: usize = 80;
pub const PREVIEW_HEIGHT: usize = 48;

/// Camera preview frame for TUI display.
#[derive(Debug, Clone)]
pub struct CameraPreview {
    /// Downsampled grayscale pixels (PREVIEW_WIDTH × PREVIEW_HEIGHT).
    pub pixels: Vec<u8>,
    /// Width of preview in pixels.
    pub width: usize,
    /// Height of preview in pixels.
    pub height: usize,
    /// QR code bounding box corners (normalized 0.0-1.0), if detected.
    /// Order: top-left, top-right, bottom-right, bottom-left.
    pub qr_bounds: Option<[(f32, f32); 4]>,
}

/// Result of a QR scan attempt.
#[derive(Debug, Clone)]
pub enum QrScanResult {
    /// Successfully decoded QR code data (raw bytes), with preview.
    Success(Vec<u8>, CameraPreview),
    /// No QR code found in frame (scanning in progress), with preview.
    Scanning(CameraPreview),
    /// QR code detected but couldn't decode (partial detection), with preview.
    Detected(CameraPreview),
    /// Error during capture or decode.
    Error(String),
}

/// Camera-based QR code reader.
///
/// Runs camera capture in a background thread to avoid blocking the UI.
pub struct QrReader {
    /// Channel to receive scan results from background thread.
    result_rx: mpsc::Receiver<QrScanResult>,
    /// Channel to send stop signal to background thread.
    stop_tx: mpsc::Sender<()>,
    /// Whether the reader is currently active.
    active: bool,
}

impl QrReader {
    /// Create and start a new QR reader.
    ///
    /// This opens the default camera and starts scanning in a background thread.
    pub fn new() -> Result<Self, String> {
        let (result_tx, result_rx) = mpsc::channel();
        let (stop_tx, stop_rx) = mpsc::channel();

        // Spawn background thread for camera capture
        thread::spawn(move || {
            if let Err(e) = camera_capture_loop(result_tx.clone(), stop_rx) {
                let _ = result_tx.send(QrScanResult::Error(e));
            }
        });

        Ok(QrReader {
            result_rx,
            stop_tx,
            active: true,
        })
    }

    /// Check if a QR code has been scanned.
    ///
    /// Returns immediately with the latest result, or None if no result yet.
    pub fn try_recv(&self) -> Option<QrScanResult> {
        self.result_rx.try_recv().ok()
    }

    /// Check if the reader is still active.
    #[allow(dead_code)]
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Stop the QR reader and release camera resources.
    pub fn stop(&mut self) {
        if self.active {
            let _ = self.stop_tx.send(());
            self.active = false;
        }
    }
}

impl Drop for QrReader {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Background camera capture loop.
fn camera_capture_loop(
    result_tx: mpsc::Sender<QrScanResult>,
    stop_rx: mpsc::Receiver<()>,
) -> Result<(), String> {
    tracing::info!("Starting camera capture for QR scanning...");

    // Try different format requests in order of preference
    // Higher resolution helps with QR code detection
    let formats_to_try = [
        // First try: 1280x720 MJPEG (HD for better QR detection)
        RequestedFormat::new::<RgbFormat>(RequestedFormatType::Closest(CameraFormat::new(
            Resolution::new(1280, 720),
            FrameFormat::MJPEG,
            30,
        ))),
        // Second try: 1280x720 YUYV
        RequestedFormat::new::<RgbFormat>(RequestedFormatType::Closest(CameraFormat::new(
            Resolution::new(1280, 720),
            FrameFormat::YUYV,
            30,
        ))),
        // Third try: 640x480 MJPEG (fallback)
        RequestedFormat::new::<RgbFormat>(RequestedFormatType::Closest(CameraFormat::new(
            Resolution::new(640, 480),
            FrameFormat::MJPEG,
            30,
        ))),
        // Fourth try: let camera choose its default format
        RequestedFormat::new::<RgbFormat>(RequestedFormatType::None),
    ];

    let mut camera = None;
    let mut last_error = String::new();

    for (i, requested) in formats_to_try.iter().enumerate() {
        tracing::info!("Trying camera format {}/{}...", i + 1, formats_to_try.len());
        match Camera::new(CameraIndex::Index(0), *requested) {
            Ok(cam) => {
                camera = Some(cam);
                break;
            }
            Err(e) => {
                last_error = format!("{}", e);
                tracing::warn!("Format {} failed: {}", i + 1, e);
            }
        }
    }

    let mut camera = camera.ok_or_else(|| {
        format!(
            "Failed to open camera with any format. Last error: {}. \
             Make sure Terminal has camera permission in System Settings → Privacy & Security → Camera",
            last_error
        )
    })?;

    camera
        .open_stream()
        .map_err(|e| format!("Failed to start camera stream: {}", e))?;

    tracing::info!(
        "Camera opened: {:?} at {:?}",
        camera.info().human_name(),
        camera.resolution()
    );

    // Capture loop
    loop {
        // Check for stop signal
        if stop_rx.try_recv().is_ok() {
            tracing::info!("QR reader stop signal received");
            break;
        }

        // Capture a frame
        let frame = match camera.frame() {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!("Frame capture error: {}", e);
                thread::sleep(Duration::from_millis(100));
                continue;
            }
        };

        // Decode the frame buffer to RGB
        let decoded = match frame.decode_image::<RgbFormat>() {
            Ok(img) => img,
            Err(e) => {
                tracing::warn!("Frame decode error: {}", e);
                thread::sleep(Duration::from_millis(100));
                continue;
            }
        };

        // Convert to grayscale for QR detection
        let width = decoded.width() as usize;
        let height = decoded.height() as usize;
        let rgb_data = decoded.into_raw();

        // Convert RGB to grayscale
        let mut gray_data = Vec::with_capacity(width * height);
        for chunk in rgb_data.chunks(3) {
            if chunk.len() == 3 {
                // Standard luminance formula: Y = 0.299*R + 0.587*G + 0.114*B
                let gray =
                    (chunk[0] as u32 * 299 + chunk[1] as u32 * 587 + chunk[2] as u32 * 114) / 1000;
                gray_data.push(gray as u8);
            }
        }

        // Create downsampled preview for TUI display
        let preview_pixels = downsample_grayscale(&gray_data, width, height, PREVIEW_WIDTH, PREVIEW_HEIGHT);

        // Try to decode QR code
        let mut decoder = rqrr::PreparedImage::prepare_from_greyscale(width, height, |x, y| {
            gray_data.get(y * width + x).copied().unwrap_or(0)
        });

        let grids = decoder.detect_grids();

        if !grids.is_empty() {
            tracing::info!("Detected {} QR grid(s) in {}x{} frame", grids.len(), width, height);
        }

        if grids.is_empty() {
            // No QR code detected - report scanning status with preview
            let preview = CameraPreview {
                pixels: preview_pixels,
                width: PREVIEW_WIDTH,
                height: PREVIEW_HEIGHT,
                qr_bounds: None,
            };
            let _ = result_tx.send(QrScanResult::Scanning(preview));
        } else {
            let mut decoded_any = false;
            let mut qr_bounds = None;

            for grid in &grids {
                // Extract bounding box from first detected grid
                if qr_bounds.is_none() {
                    qr_bounds = Some(extract_qr_bounds(grid, width, height));
                }

                match grid.decode() {
                    Ok((meta, content)) => {
                        // Convert String content to bytes
                        let bytes = content.into_bytes();
                        tracing::info!(
                            "QR decoded: {} bytes, ECC={:?}, version={:?}",
                            bytes.len(),
                            meta.ecc_level,
                            meta.version
                        );
                        if !bytes.is_empty() {
                            tracing::info!(
                                "First bytes: {:02x?}",
                                &bytes[..bytes.len().min(10)]
                            );
                        }
                        let preview = CameraPreview {
                            pixels: preview_pixels.clone(),
                            width: PREVIEW_WIDTH,
                            height: PREVIEW_HEIGHT,
                            qr_bounds,
                        };
                        let _ = result_tx.send(QrScanResult::Success(bytes, preview));
                        decoded_any = true;
                        // Continue scanning - the caller decides when to stop
                    }
                    Err(e) => {
                        tracing::warn!("QR decode error: {:?}", e);
                    }
                }
            }
            // QR grid detected but couldn't decode
            if !decoded_any {
                let preview = CameraPreview {
                    pixels: preview_pixels,
                    width: PREVIEW_WIDTH,
                    height: PREVIEW_HEIGHT,
                    qr_bounds,
                };
                let _ = result_tx.send(QrScanResult::Detected(preview));
            }
        }

        // Small delay to not overwhelm CPU
        thread::sleep(Duration::from_millis(50));
    }

    tracing::info!("Camera capture loop ended");
    Ok(())
}

/// Downsample a grayscale image using area averaging.
fn downsample_grayscale(
    src: &[u8],
    src_width: usize,
    src_height: usize,
    dst_width: usize,
    dst_height: usize,
) -> Vec<u8> {
    let mut dst = Vec::with_capacity(dst_width * dst_height);

    let x_ratio = src_width as f32 / dst_width as f32;
    let y_ratio = src_height as f32 / dst_height as f32;

    for dst_y in 0..dst_height {
        for dst_x in 0..dst_width {
            // Map destination pixel to source region
            let src_x = (dst_x as f32 * x_ratio) as usize;
            let src_y = (dst_y as f32 * y_ratio) as usize;

            // Simple nearest-neighbor sampling (fast)
            let idx = src_y * src_width + src_x;
            let pixel = src.get(idx).copied().unwrap_or(128);
            dst.push(pixel);
        }
    }

    dst
}

/// Extract QR code bounding box as normalized coordinates (0.0-1.0).
fn extract_qr_bounds<G>(
    grid: &rqrr::Grid<G>,
    img_width: usize,
    img_height: usize,
) -> [(f32, f32); 4] {
    // rqrr::Grid has bounds field that contains the 4 corners
    let bounds = &grid.bounds;

    // Bounds are in pixel coordinates, normalize to 0.0-1.0
    let normalize = |p: rqrr::Point| {
        (
            p.x as f32 / img_width as f32,
            p.y as f32 / img_height as f32,
        )
    };

    [
        normalize(bounds[0]), // top-left
        normalize(bounds[1]), // top-right
        normalize(bounds[2]), // bottom-right
        normalize(bounds[3]), // bottom-left
    ]
}

/// Decode a QR code from raw image bytes (for testing or file-based input).
#[allow(dead_code)]
pub fn decode_qr_from_image(image_data: &[u8]) -> Result<Vec<u8>, String> {
    let img =
        image::load_from_memory(image_data).map_err(|e| format!("Failed to load image: {}", e))?;

    let gray = img.to_luma8();
    let width = gray.width() as usize;
    let height = gray.height() as usize;

    let mut decoder = rqrr::PreparedImage::prepare_from_greyscale(width, height, |x, y| {
        gray.get_pixel(x as u32, y as u32).0[0]
    });

    let grids = decoder.detect_grids();

    for grid in grids {
        if let Ok((_, content)) = grid.decode() {
            return Ok(content.into_bytes());
        }
    }

    Err("No QR code found in image".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_qr_from_image() {
        // This test would require a sample QR code image
        // For now, just verify the function compiles and handles errors
        let result = decode_qr_from_image(&[]);
        assert!(result.is_err());
    }
}
