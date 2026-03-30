//! Event bus with typed events and async subscribers.
//!
//! Port from Python `core/events.py`.
//! All pipeline actions are modeled as events that can be
//! subscribed to, logged, and replayed.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::warn;

/// Pipeline event types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    // Discovery
    DiscoveryStart,
    DiscoveryComplete,
    // Analysis
    AnalysisStart,
    AnalysisComplete,
    // Generation
    GenerationStart,
    GenerationComplete,
    // PR lifecycle
    PrCreated,
    PrClosed,
    PrMerged,
    // Pipeline
    PipelineStart,
    PipelineComplete,
    PipelineError,
    // Hunt mode
    HuntRoundStart,
    HuntRoundComplete,
    HuntRepoStart,
    HuntRepoComplete,
    HuntRepoSkip,
    // Memory
    MemoryStore,
    MemoryRecall,
}

impl fmt::Display for EventType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::DiscoveryStart => "discovery.start",
            Self::DiscoveryComplete => "discovery.complete",
            Self::AnalysisStart => "analysis.start",
            Self::AnalysisComplete => "analysis.complete",
            Self::GenerationStart => "generation.start",
            Self::GenerationComplete => "generation.complete",
            Self::PrCreated => "pr.created",
            Self::PrClosed => "pr.closed",
            Self::PrMerged => "pr.merged",
            Self::PipelineStart => "pipeline.start",
            Self::PipelineComplete => "pipeline.complete",
            Self::PipelineError => "pipeline.error",
            Self::HuntRoundStart => "hunt.round_start",
            Self::HuntRoundComplete => "hunt.round_complete",
            Self::HuntRepoStart => "hunt.repo_start",
            Self::HuntRepoComplete => "hunt.repo_complete",
            Self::HuntRepoSkip => "hunt.repo_skip",
            Self::MemoryStore => "memory.store",
            Self::MemoryRecall => "memory.recall",
        };
        write!(f, "{}", s)
    }
}

/// Immutable event emitted by pipeline stages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    #[serde(rename = "type")]
    pub event_type: EventType,
    #[serde(default)]
    pub data: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub source: String,
    pub timestamp: DateTime<Utc>,
    pub event_id: String,
}

impl Event {
    pub fn new(event_type: EventType, source: &str) -> Self {
        let now = Utc::now();
        Self {
            event_type,
            data: HashMap::new(),
            source: source.to_string(),
            timestamp: now,
            event_id: now.format("%Y%m%d%H%M%S%f").to_string(),
        }
    }

    pub fn with_data(mut self, key: &str, value: impl Into<serde_json::Value>) -> Self {
        self.data.insert(key.to_string(), value.into());
        self
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// Central event bus for pipeline observability.
///
/// Uses tokio broadcast channel for async event dispatch.
pub struct EventBus {
    sender: broadcast::Sender<Event>,
    history: Arc<RwLock<Vec<Event>>>,
    max_history: usize,
}

impl EventBus {
    pub fn new(max_history: usize) -> Self {
        let (sender, _) = broadcast::channel(256);
        Self {
            sender,
            history: Arc::new(RwLock::new(Vec::new())),
            max_history,
        }
    }

    /// Subscribe to events. Returns a receiver.
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.sender.subscribe()
    }

    /// Emit an event.
    pub async fn emit(&self, event: Event) {
        // Store in history
        {
            let mut history = self.history.write().await;
            history.push(event.clone());
            if history.len() > self.max_history {
                let drain = history.len() - self.max_history;
                history.drain(..drain);
            }
        }

        // Broadcast (ok to fail if no receivers)
        let _ = self.sender.send(event);
    }

    /// Get event history, optionally filtered.
    pub async fn history(
        &self,
        event_type: Option<EventType>,
        limit: usize,
    ) -> Vec<Event> {
        let history = self.history.read().await;
        let events: Vec<_> = if let Some(et) = event_type {
            history.iter().filter(|e| e.event_type == et).cloned().collect()
        } else {
            history.clone()
        };
        events.into_iter().rev().take(limit).collect()
    }

    /// Clear all history.
    pub async fn clear_history(&self) {
        self.history.write().await.clear();
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(1000)
    }
}

/// JSONL file logger subscriber.
pub struct FileEventLogger {
    path: PathBuf,
}

impl FileEventLogger {
    pub fn new(path: &Path) -> Self {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        Self {
            path: path.to_path_buf(),
        }
    }

    /// Append event as JSON line to file.
    pub fn handle(&self, event: &Event) {
        use std::io::Write;
        match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            Ok(mut file) => {
                let _ = writeln!(file, "{}", event.to_json());
            }
            Err(e) => {
                warn!(error = %e, "Failed to write event to {:?}", self.path);
            }
        }
    }

    /// Spawn a background task that logs all events from the bus.
    pub fn spawn_logger(self, bus: &EventBus) -> tokio::task::JoinHandle<()> {
        let mut rx = bus.subscribe();
        tokio::spawn(async move {
            while let Ok(event) = rx.recv().await {
                self.handle(&event);
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_event_bus_emit_and_history() {
        let bus = EventBus::default();
        let event = Event::new(EventType::PipelineStart, "test")
            .with_data("dry_run", false);

        bus.emit(event).await;

        let history = bus.history(None, 10).await;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].event_type, EventType::PipelineStart);
    }

    #[tokio::test]
    async fn test_event_bus_filtered_history() {
        let bus = EventBus::default();

        bus.emit(Event::new(EventType::PipelineStart, "test")).await;
        bus.emit(Event::new(EventType::DiscoveryStart, "test")).await;
        bus.emit(Event::new(EventType::PipelineComplete, "test")).await;

        let all = bus.history(None, 100).await;
        assert_eq!(all.len(), 3);

        let pipeline_only = bus.history(Some(EventType::PipelineStart), 100).await;
        assert_eq!(pipeline_only.len(), 1);
    }

    #[tokio::test]
    async fn test_event_bus_max_history() {
        let bus = EventBus::new(5);
        for i in 0..10 {
            bus.emit(
                Event::new(EventType::AnalysisStart, "test")
                    .with_data("i", i as i64),
            )
            .await;
        }

        let history = bus.history(None, 100).await;
        assert_eq!(history.len(), 5);
    }

    #[test]
    fn test_event_serialization() {
        let event = Event::new(EventType::PrCreated, "pr.manager")
            .with_data("pr_number", 42)
            .with_data("repo", "test/repo");

        let json = event.to_json();
        assert!(json.contains("pr_created") || json.contains("pr.created") || json.contains("PrCreated"));
        assert!(json.contains("42"));
    }

    #[test]
    fn test_event_type_display() {
        assert_eq!(EventType::PipelineStart.to_string(), "pipeline.start");
        assert_eq!(EventType::PrCreated.to_string(), "pr.created");
        assert_eq!(EventType::HuntRoundStart.to_string(), "hunt.round_start");
    }
}
