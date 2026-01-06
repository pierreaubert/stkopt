//! Terminal event handling.

use color_eyre::Result;
use crossterm::event::{self, Event as CrosstermEvent, KeyEvent};
use std::time::Duration;
use tokio::sync::mpsc;

/// Terminal events.
#[derive(Debug)]
pub enum Event {
    /// Periodic tick for updates.
    Tick,
    /// Keyboard input.
    Key(KeyEvent),
    /// Terminal resize (width, height).
    #[allow(dead_code)]
    Resize(u16, u16),
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
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if event_tx.send(Event::Tick).is_err() {
                            break;
                        }
                    }
                    _ = tokio::task::spawn_blocking(|| {
                        event::poll(Duration::from_millis(50))
                    }) => {
                        if let Ok(true) = event::poll(Duration::ZERO)
                            && let Ok(evt) = event::read()
                        {
                            let event = match evt {
                                CrosstermEvent::Key(key) => Some(Event::Key(key)),
                                CrosstermEvent::Resize(w, h) => Some(Event::Resize(w, h)),
                                _ => None,
                            };
                            if let Some(event) = event
                                && event_tx.send(event).is_err()
                            {
                                break;
                            }
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
