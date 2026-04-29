use crate::app_paths::AppPaths;
use crate::live_client::{ActivePlayerSnapshot, LiveClientDataApi, LiveClientError};
use crate::models::TelemetryEventInput;
use crate::settings::load_or_create_settings;
use crate::state_machine::{
    AssistantState, AssistantStateMachine, AugmentChoice, LivePlayerSnapshot, PanelState,
    PauseReason, StateMachineInput, StateTransitionEvent,
};
use crate::telemetry;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tauri::AppHandle;

const RECENT_EVENT_LIMIT: usize = 40;
const MIN_LISTEN_INTERVAL_MS: u64 = 2_500;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibratedPanelSnapshot {
    pub panel_state: PanelState,
    pub choices: Vec<AugmentChoice>,
    pub selected_slot: Option<u8>,
}

impl Default for CalibratedPanelSnapshot {
    fn default() -> Self {
        Self {
            panel_state: PanelState::Collapsed,
            choices: Vec::new(),
            selected_slot: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeTriggerRequest {
    pub panel_snapshot: Option<CalibratedPanelSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeTriggerEvent {
    Manual,
    LowFrequencyPoll,
    ListenerStarted,
    ListenerStopped,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSlotChange {
    pub slot: u8,
    pub previous_value: Option<String>,
    pub next_value: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStructuredEvent {
    pub trace_id: String,
    pub occurred_at: String,
    pub trigger_event: RuntimeTriggerEvent,
    pub champion_name: Option<String>,
    pub level: Option<u8>,
    pub pending_tiers: Vec<u8>,
    pub state_events: Vec<StateTransitionEvent>,
    pub slot_changes: Vec<RuntimeSlotChange>,
    pub error_code: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeLoopSnapshot {
    pub listening: bool,
    pub state: AssistantState,
    pub panel_snapshot: CalibratedPanelSnapshot,
    pub recent_events: Vec<RuntimeStructuredEvent>,
    pub last_error_code: Option<String>,
}

#[derive(Debug)]
pub struct RuntimeOrchestratorHandle {
    inner: Arc<Mutex<RuntimeOrchestrator>>,
    listening: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl Default for RuntimeOrchestratorHandle {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(RuntimeOrchestrator::new())),
            listening: Arc::new(AtomicBool::new(false)),
            worker: Mutex::new(None),
        }
    }
}

impl RuntimeOrchestratorHandle {
    pub fn snapshot(&self) -> Result<RuntimeLoopSnapshot, String> {
        let orchestrator = self
            .inner
            .lock()
            .map_err(|_| "运行时编排器状态锁已损坏".to_string())?;
        Ok(orchestrator.snapshot(self.listening.load(Ordering::SeqCst)))
    }

    pub fn trigger_once(
        &self,
        app: &AppHandle,
        request: RuntimeTriggerRequest,
    ) -> Result<RuntimeLoopSnapshot, String> {
        let paths = AppPaths::from_app(app)?;
        paths.ensure_all()?;
        let mut orchestrator = self
            .inner
            .lock()
            .map_err(|_| "运行时编排器状态锁已损坏".to_string())?;
        orchestrator.apply_panel_snapshot(request.panel_snapshot);
        let live_result = LiveClientDataApi::new().fetch_active_player();
        orchestrator.tick(&paths, RuntimeTriggerEvent::Manual, live_result)?;
        Ok(orchestrator.snapshot(self.listening.load(Ordering::SeqCst)))
    }

    pub fn start_listener(
        &self,
        app: &AppHandle,
        request: RuntimeTriggerRequest,
    ) -> Result<RuntimeLoopSnapshot, String> {
        let paths = AppPaths::from_app(app)?;
        paths.ensure_all()?;
        let settings = load_or_create_settings(&paths)?;
        let interval_ms = settings
            .capture
            .poll_interval_ms
            .max(MIN_LISTEN_INTERVAL_MS);

        {
            let mut orchestrator = self
                .inner
                .lock()
                .map_err(|_| "运行时编排器状态锁已损坏".to_string())?;
            orchestrator.apply_panel_snapshot(request.panel_snapshot);
            orchestrator.record_control_event(&paths, RuntimeTriggerEvent::ListenerStarted)?;
        }

        if self.listening.swap(true, Ordering::SeqCst) {
            return self.snapshot();
        }

        let inner = Arc::clone(&self.inner);
        let listening = Arc::clone(&self.listening);
        let worker_paths = paths.clone();
        let handle = thread::spawn(move || {
            while listening.load(Ordering::SeqCst) {
                let live_result = LiveClientDataApi::new().fetch_active_player();
                if let Ok(mut orchestrator) = inner.lock() {
                    let _ = orchestrator.tick(
                        &worker_paths,
                        RuntimeTriggerEvent::LowFrequencyPoll,
                        live_result,
                    );
                }
                thread::sleep(Duration::from_millis(interval_ms));
            }
        });

        let mut worker_slot = self
            .worker
            .lock()
            .map_err(|_| "运行时监听线程锁已损坏".to_string())?;
        *worker_slot = Some(handle);

        self.snapshot()
    }

    pub fn stop_listener(&self, app: &AppHandle) -> Result<RuntimeLoopSnapshot, String> {
        self.listening.store(false, Ordering::SeqCst);
        if let Some(handle) = self
            .worker
            .lock()
            .map_err(|_| "运行时监听线程锁已损坏".to_string())?
            .take()
        {
            handle
                .join()
                .map_err(|_| "运行时监听线程退出失败".to_string())?;
        }

        let paths = AppPaths::from_app(app)?;
        paths.ensure_all()?;
        let mut orchestrator = self
            .inner
            .lock()
            .map_err(|_| "运行时编排器状态锁已损坏".to_string())?;
        orchestrator.record_control_event(&paths, RuntimeTriggerEvent::ListenerStopped)?;
        Ok(orchestrator.snapshot(false))
    }
}

#[derive(Debug)]
struct RuntimeOrchestrator {
    machine: AssistantStateMachine,
    panel_snapshot: CalibratedPanelSnapshot,
    recent_events: VecDeque<RuntimeStructuredEvent>,
    last_error_code: Option<String>,
}

impl RuntimeOrchestrator {
    fn new() -> Self {
        Self {
            machine: AssistantStateMachine::new(),
            panel_snapshot: CalibratedPanelSnapshot::default(),
            recent_events: VecDeque::new(),
            last_error_code: None,
        }
    }

    fn snapshot(&self, listening: bool) -> RuntimeLoopSnapshot {
        RuntimeLoopSnapshot {
            listening,
            state: self.machine.state().clone(),
            panel_snapshot: self.panel_snapshot.clone(),
            recent_events: self.recent_events.iter().cloned().collect(),
            last_error_code: self.last_error_code.clone(),
        }
    }

    fn apply_panel_snapshot(&mut self, panel_snapshot: Option<CalibratedPanelSnapshot>) {
        if let Some(panel_snapshot) = panel_snapshot {
            self.panel_snapshot = panel_snapshot;
        }
    }

    fn tick(
        &mut self,
        paths: &AppPaths,
        trigger_event: RuntimeTriggerEvent,
        live_result: Result<ActivePlayerSnapshot, LiveClientError>,
    ) -> Result<(), String> {
        let start = Instant::now();
        let (player, pause_reason, error_code, error_message) = match live_result {
            Ok(player) => (
                Some(LivePlayerSnapshot {
                    champion_name: player.champion_name,
                    level: player.level,
                }),
                None,
                None,
                None,
            ),
            Err(error) => (
                None,
                Some(error.pause_reason()),
                Some(error.error_code().to_string()),
                Some(error.to_string()),
            ),
        };

        let input = StateMachineInput {
            player: player.clone(),
            panel_state: self.panel_snapshot.panel_state,
            choices: self.panel_snapshot.choices.clone(),
            selected_slot: self.panel_snapshot.selected_slot,
            pause_reason,
        };
        let state_events = self.machine.apply(input);
        self.last_error_code = error_code.clone();

        let message = match error_message {
            Some(message) => format!("运行时编排暂停: {message}"),
            None => "运行时编排完成一次状态刷新".to_string(),
        };
        self.record_event(
            paths,
            trigger_event,
            player,
            state_events,
            error_code,
            message,
            start.elapsed().as_millis(),
        )
    }

    fn record_control_event(
        &mut self,
        paths: &AppPaths,
        trigger_event: RuntimeTriggerEvent,
    ) -> Result<(), String> {
        self.record_event(
            paths,
            trigger_event,
            self.machine.state().player.clone(),
            Vec::new(),
            None,
            match trigger_event {
                RuntimeTriggerEvent::ListenerStarted => "低频监听已启动".to_string(),
                RuntimeTriggerEvent::ListenerStopped => "低频监听已停止".to_string(),
                _ => "运行时控制事件".to_string(),
            },
            0,
        )
    }

    fn record_event(
        &mut self,
        paths: &AppPaths,
        trigger_event: RuntimeTriggerEvent,
        player: Option<LivePlayerSnapshot>,
        state_events: Vec<StateTransitionEvent>,
        error_code: Option<String>,
        message: String,
        duration_ms: u128,
    ) -> Result<(), String> {
        let log_player = player
            .clone()
            .or_else(|| self.machine.state().player.clone());
        let pending_tiers = self.machine.state().pending_tiers.clone();
        let slot_changes = state_events
            .iter()
            .filter_map(|event| {
                event.slot.map(|slot| RuntimeSlotChange {
                    slot,
                    previous_value: event.previous_value.clone(),
                    next_value: event.next_value.clone(),
                })
            })
            .collect::<Vec<_>>();

        let log_payload = serde_json::json!({
            "triggerEvent": trigger_event,
            "championName": log_player.as_ref().map(|player| player.champion_name.as_str()),
            "level": log_player.as_ref().map(|player| player.level),
            "pendingTiers": pending_tiers.clone(),
            "panelState": self.panel_snapshot.panel_state,
            "slotChanges": slot_changes.clone(),
            "stateEvents": state_events.clone(),
            "errorCode": error_code.clone(),
        });

        let telemetry_event = telemetry::write_event(
            paths,
            TelemetryEventInput {
                stage: "runtime-orchestrator".to_string(),
                input_summary: format!("触发事件 {:?}", trigger_event),
                output_summary: format!(
                    "英雄 {:?}，等级 {:?}，待处理档位 {:?}",
                    log_player
                        .as_ref()
                        .map(|player| player.champion_name.as_str()),
                    log_player.as_ref().map(|player| player.level),
                    self.machine.state().pending_tiers
                ),
                duration_ms,
                level: if error_code.is_some() {
                    "warn".to_string()
                } else {
                    "info".to_string()
                },
                error_code: error_code.clone(),
                message: log_payload.to_string(),
            },
        )?;

        let event = RuntimeStructuredEvent {
            trace_id: telemetry_event.trace_id,
            occurred_at: Utc::now().to_rfc3339(),
            trigger_event,
            champion_name: log_player
                .as_ref()
                .map(|player| player.champion_name.clone()),
            level: log_player.as_ref().map(|player| player.level),
            pending_tiers: self.machine.state().pending_tiers.clone(),
            state_events,
            slot_changes,
            error_code,
            message,
        };
        self.recent_events.push_front(event);
        while self.recent_events.len() > RECENT_EVENT_LIMIT {
            self.recent_events.pop_back();
        }
        Ok(())
    }
}

impl LiveClientError {
    fn error_code(&self) -> &'static str {
        match self {
            Self::Http(_) => "HEX-LIVE-CLIENT-UNAVAILABLE",
            Self::InvalidPayload(_) => "HEX-LIVE-CLIENT-PAYLOAD",
        }
    }

    fn pause_reason(&self) -> PauseReason {
        match self {
            Self::Http(_) => PauseReason::LiveClientUnavailable,
            Self::InvalidPayload(_) => PauseReason::InvalidLiveClientData,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn manual_tick_records_pending_tiers_and_slot_changes() {
        let paths = temp_paths("runtime-orchestrator");
        paths.ensure_all().expect("应能创建测试目录");
        let mut orchestrator = RuntimeOrchestrator::new();
        orchestrator.apply_panel_snapshot(Some(CalibratedPanelSnapshot {
            panel_state: PanelState::Expanded,
            choices: vec![
                AugmentChoice {
                    slot: 1,
                    augment_id: "prismatic-ticket".to_string(),
                },
                AugmentChoice {
                    slot: 2,
                    augment_id: "build-a-bud".to_string(),
                },
            ],
            selected_slot: None,
        }));

        orchestrator
            .tick(
                &paths,
                RuntimeTriggerEvent::Manual,
                Ok(ActivePlayerSnapshot {
                    champion_name: "Ahri".to_string(),
                    level: 11,
                }),
            )
            .expect("手动触发应成功");

        let snapshot = orchestrator.snapshot(false);
        assert_eq!(snapshot.state.pending_tier, Some(3));
        assert_eq!(snapshot.state.pending_tiers, vec![3, 7, 11]);
        assert_eq!(
            snapshot.recent_events[0].champion_name.as_deref(),
            Some("Ahri")
        );
        assert_eq!(snapshot.recent_events[0].slot_changes.len(), 2);

        let _ = std::fs::remove_dir_all(paths.root);
    }

    fn temp_paths(label: &str) -> AppPaths {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("系统时间应可用")
            .as_micros();
        let root = std::env::temp_dir().join(format!("hex-assistant-{label}-{suffix}"));
        AppPaths {
            config: root.join("config"),
            calibration: root.join("calibration"),
            logs: root.join("logs"),
            samples: root.join("samples"),
            ocr_replay: root.join("ocr-replay"),
            captures: root.join("captures"),
            reports: root.join("reports"),
            cache: root.join("cache"),
            root,
        }
    }
}
