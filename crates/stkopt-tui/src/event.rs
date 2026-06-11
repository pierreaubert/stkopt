//! Terminal event handling.

use color_eyre::Result;
use crossterm::event::{Event as CrosstermEvent, EventStream, KeyEvent, KeyEventKind};
use futures::StreamExt;
use std::time::Duration;
use tokio::sync::mpsc;

/// Terminal events.
#[derive(Debug)]
pub enum Event {
    /// Periodic tick for updates.
    Tick,
    /// Keyboard input.
    Key(KeyEvent),
}

/// Event handler that polls for terminal events.
pub struct EventHandler {
    /// Event receiver.
    rx: mpsc::UnboundedReceiver<Event>,
    /// Event sender (kept to prevent channel closing).
    #[allow(dead_code)]
    tx: mpsc::UnboundedSender<Event>,
}

impl EventHandler {
    /// Create a new event handler with the given tick rate in milliseconds.
    pub fn new(tick_rate_ms: u64) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let tick_rate = Duration::from_millis(tick_rate_ms);

        let event_tx = tx.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tick_rate);
            let mut event_stream = EventStream::new();
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if event_tx.send(Event::Tick).is_err() {
                            break;
                        }
                    }
                    maybe_event = event_stream.next() => {
                        match maybe_event {
                            Some(Ok(evt)) => {
                                let event = match evt {
                                    CrosstermEvent::Key(key) if key.kind == KeyEventKind::Press => {
                                        Some(Event::Key(key))
                                    }
                                    CrosstermEvent::Key(_) => None, // Ignore Release/Repeat
                                    // Resize is handled automatically by ratatui
                                    _ => None,
                                };
                                if let Some(event) = event
                                    && event_tx.send(event).is_err()
                                {
                                    break;
                                }
                            }
                            Some(Err(_)) | None => break,
                        }
                    }
                }
            }
        });

        Self { rx, tx }
    }

    /// Get the next event.
    pub async fn next(&mut self) -> Result<Event> {
        self.rx
            .recv()
            .await
            .ok_or_else(|| color_eyre::eyre::eyre!("Event channel closed"))
    }
}
