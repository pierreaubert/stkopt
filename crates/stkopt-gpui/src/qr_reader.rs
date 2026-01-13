//! QR code reader using camera capture for GPUI.
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

/// Preview dimensions for GPUI display.
pub const PREVIEW_WIDTH: usize = 320;
pub const PREVIEW_HEIGHT: usize = 240;

/// Camera preview frame for GPUI display.
#[derive(Debug, Clone)]
pub struct CameraPreview {
    /// RGB pixels (PREVIEW_WIDTH x PREVIEW_HEIGHT x 3).
    pub rgb_pixels: Vec<u8>,
    /// Width of preview in pixels.
    pub width: usize,
    /// Height of preview in pixels.
    pub height: usize,
    /// QR code bounding box corners (normalized 0.0-1.0), if detected.
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

    // Try different format requests
    let formats_to_try = [
        RequestedFormat::new::<RgbFormat>(RequestedFormatType::Closest(CameraFormat::new(
            Resolution::new(1280, 720),
            FrameFormat::MJPEG,
            30,
        ))),
        RequestedFormat::new::<RgbFormat>(RequestedFormatType::Closest(CameraFormat::new(
            Resolution::new(1280, 720),
            FrameFormat::YUYV,
            30,
        ))),
        RequestedFormat::new::<RgbFormat>(RequestedFormatType::Closest(CameraFormat::new(
            Resolution::new(640, 480),
            FrameFormat::MJPEG,
            30,
        ))),
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
            "Failed to open camera. Last error: {}. \
             Make sure the app has camera permission in System Settings.",
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

        let width = decoded.width() as usize;
        let height = decoded.height() as usize;
        let rgb_data = decoded.into_raw();

        // Convert RGB to grayscale for QR detection
        let mut gray_data = Vec::with_capacity(width * height);
        for chunk in rgb_data.chunks(3) {
            if chunk.len() == 3 {
                let gray =
                    (chunk[0] as u32 * 299 + chunk[1] as u32 * 587 + chunk[2] as u32 * 114) / 1000;
                gray_data.push(gray as u8);
            }
        }

        // Create downsampled preview
        let preview_rgb = downsample_rgb(&rgb_data, width, height, PREVIEW_WIDTH, PREVIEW_HEIGHT);

        // Try to decode QR code
        let mut decoder = rqrr::PreparedImage::prepare_from_greyscale(width, height, |x, y| {
            gray_data.get(y * width + x).copied().unwrap_or(0)
        });

        let grids = decoder.detect_grids();

        if grids.is_empty() {
            let preview = CameraPreview {
                rgb_pixels: preview_rgb,
                width: PREVIEW_WIDTH,
                height: PREVIEW_HEIGHT,
                qr_bounds: None,
            };
            let _ = result_tx.send(QrScanResult::Scanning(preview));
        } else {
            let mut decoded_any = false;
            let mut qr_bounds = None;

            for grid in &grids {
                if qr_bounds.is_none() {
                    qr_bounds = Some(extract_qr_bounds(grid, width, height));
                }

                match grid.decode() {
                    Ok((meta, content)) => {
                        let bytes = content.into_bytes();
                        tracing::info!(
                            "QR decoded: {} bytes, ECC={:?}",
                            bytes.len(),
                            meta.ecc_level
                        );
                        let preview = CameraPreview {
                            rgb_pixels: preview_rgb.clone(),
                            width: PREVIEW_WIDTH,
                            height: PREVIEW_HEIGHT,
                            qr_bounds,
                        };
                        let _ = result_tx.send(QrScanResult::Success(bytes, preview));
                        decoded_any = true;
                    }
                    Err(e) => {
                        tracing::warn!("QR decode error: {:?}", e);
                    }
                }
            }

            if !decoded_any {
                let preview = CameraPreview {
                    rgb_pixels: preview_rgb,
                    width: PREVIEW_WIDTH,
                    height: PREVIEW_HEIGHT,
                    qr_bounds,
                };
                let _ = result_tx.send(QrScanResult::Detected(preview));
            }
        }

        thread::sleep(Duration::from_millis(50));
    }

    tracing::info!("Camera capture loop ended");
    Ok(())
}

/// Downsample an RGB image.
fn downsample_rgb(
    src: &[u8],
    src_width: usize,
    src_height: usize,
    dst_width: usize,
    dst_height: usize,
) -> Vec<u8> {
    let mut dst = Vec::with_capacity(dst_width * dst_height * 3);

    let x_ratio = src_width as f32 / dst_width as f32;
    let y_ratio = src_height as f32 / dst_height as f32;

    for dst_y in 0..dst_height {
        for dst_x in 0..dst_width {
            let src_x = (dst_x as f32 * x_ratio) as usize;
            let src_y = (dst_y as f32 * y_ratio) as usize;
            let idx = (src_y * src_width + src_x) * 3;

            if idx + 2 < src.len() {
                dst.push(src[idx]);
                dst.push(src[idx + 1]);
                dst.push(src[idx + 2]);
            } else {
                dst.push(128);
                dst.push(128);
                dst.push(128);
            }
        }
    }

    dst
}

/// Extract QR code bounding box as normalized coordinates.
fn extract_qr_bounds<G>(
    grid: &rqrr::Grid<G>,
    img_width: usize,
    img_height: usize,
) -> [(f32, f32); 4] {
    let bounds = &grid.bounds;

    let normalize = |p: rqrr::Point| {
        (
            p.x as f32 / img_width as f32,
            p.y as f32 / img_height as f32,
        )
    };

    [
        normalize(bounds[0]),
        normalize(bounds[1]),
        normalize(bounds[2]),
        normalize(bounds[3]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_downsample_rgb() {
        // Test basic downsampling doesn't panic
        let src = vec![255u8; 640 * 480 * 3];
        let result = downsample_rgb(&src, 640, 480, 320, 240);
        assert_eq!(result.len(), 320 * 240 * 3);
    }
}
