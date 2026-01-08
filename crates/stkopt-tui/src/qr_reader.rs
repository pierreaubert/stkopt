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

/// Result of a QR scan attempt.
#[derive(Debug, Clone)]
pub enum QrScanResult {
    /// Successfully decoded QR code data (raw bytes).
    Success(Vec<u8>),
    /// No QR code found in frame (scanning in progress).
    Scanning,
    /// QR code detected but couldn't decode (partial detection).
    Detected,
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

    // Request 640x480 resolution - sufficient for QR codes and fast to process
    let format = CameraFormat::new(Resolution::new(640, 480), FrameFormat::MJPEG, 30);
    let requested = RequestedFormat::new::<RgbFormat>(RequestedFormatType::Closest(format));

    // Open default camera (index 0)
    let mut camera = Camera::new(CameraIndex::Index(0), requested)
        .map_err(|e| format!("Failed to open camera: {}", e))?;

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

        // Try to decode QR code
        let mut decoder = rqrr::PreparedImage::prepare_from_greyscale(width, height, |x, y| {
            gray_data.get(y * width + x).copied().unwrap_or(0)
        });

        let grids = decoder.detect_grids();

        if grids.is_empty() {
            // No QR code detected - report scanning status
            let _ = result_tx.send(QrScanResult::Scanning);
        } else {
            let mut decoded_any = false;
            for grid in grids {
                match grid.decode() {
                    Ok((_, content)) => {
                        // Convert String content to bytes
                        let bytes = content.into_bytes();
                        tracing::info!("QR code detected: {} bytes", bytes.len());
                        let _ = result_tx.send(QrScanResult::Success(bytes));
                        decoded_any = true;
                        // Continue scanning - the caller decides when to stop
                    }
                    Err(e) => {
                        tracing::debug!("QR decode error: {:?}", e);
                    }
                }
            }
            // QR grid detected but couldn't decode
            if !decoded_any {
                let _ = result_tx.send(QrScanResult::Detected);
            }
        }

        // Small delay to not overwhelm CPU
        thread::sleep(Duration::from_millis(50));
    }

    tracing::info!("Camera capture loop ended");
    Ok(())
}

/// Decode a QR code from raw image bytes (for testing or file-based input).
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
