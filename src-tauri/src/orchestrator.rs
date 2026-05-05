use crate::app_paths::AppPaths;
use crate::live_client::{LiveClientDataApi, LiveClientError, ResolvedPlayerSnapshot};
use crate::models::TelemetryEventInput;
use crate::overlay::{self, OverlaySlotData, RuntimeOverlaySyncReport};
use crate::settings::load_or_create_settings;
use crate::state_machine::{
    AssistantState, AssistantStateMachine, AugmentChoice, LivePlayerSnapshot, PanelState,
    PauseReason, StateMachineInput, StateTransitionEvent,
};
use crate::telemetry;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
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
const ALLOWED_GAME_MODES: &[&str] = &["KIWI"];
const MODE_MISMATCH_ERROR_CODE: &str = "HEX-LIVE-CLIENT-MODE-MISMATCH";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeLiveClientContext {
    pub active_player_name: Option<String>,
    pub resolved_champion_name: Option<String>,
    pub resolved_level: Option<u8>,
    pub game_mode: Option<String>,
    pub game_time: Option<f64>,
    pub source_endpoint: Option<String>,
    pub fallback_used: Option<bool>,
}

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
        let live_result = LiveClientDataApi::new().fetch_resolved_player_snapshot();
        let overlay_plan = orchestrator.tick(&paths, RuntimeTriggerEvent::Manual, live_result)?;
        orchestrator.sync_overlay(app, &paths, RuntimeTriggerEvent::Manual, &overlay_plan);
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
        let worker_app = app.clone();
        let handle = thread::spawn(move || {
            while listening.load(Ordering::SeqCst) {
                let live_result = LiveClientDataApi::new().fetch_resolved_player_snapshot();
                if let Ok(mut orchestrator) = inner.lock() {
                    if let Ok(overlay_plan) = orchestrator.tick(
                        &worker_paths,
                        RuntimeTriggerEvent::LowFrequencyPoll,
                        live_result,
                    ) {
                        orchestrator.sync_overlay(
                            &worker_app,
                            &worker_paths,
                            RuntimeTriggerEvent::LowFrequencyPoll,
                            &overlay_plan,
                        );
                    }
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
        orchestrator.sync_overlay(
            app,
            &paths,
            RuntimeTriggerEvent::ListenerStopped,
            &RuntimeOverlayPlan::Hide {
                reason: "监听已停止，自动隐藏 Overlay".to_string(),
            },
        );
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "action", rename_all = "camelCase")]
enum RuntimeOverlayPlan {
    Show {
        reason: String,
        slots: Vec<OverlaySlotData>,
    },
    Hide {
        reason: String,
    },
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
        live_result: Result<ResolvedPlayerSnapshot, LiveClientError>,
    ) -> Result<RuntimeOverlayPlan, String> {
        let start = Instant::now();
        let live_client_result_category = match &live_result {
            Ok(_) => "success".to_string(),
            Err(error) => error.result_category().to_string(),
        };
        let (
            player,
            live_client_context,
            pause_reason,
            error_code,
            error_message,
            live_client_details,
        ) = match live_result {
            Ok(snapshot) => {
                let live_client_context = RuntimeLiveClientContext::from_snapshot(&snapshot);
                if !is_allowed_game_mode(snapshot.game_mode.as_deref()) {
                    let mode = snapshot
                        .game_mode
                        .clone()
                        .unwrap_or_else(|| "未知模式".to_string());
                    (
                        None,
                        live_client_context,
                        Some(PauseReason::UnsupportedGameMode),
                        Some(MODE_MISMATCH_ERROR_CODE.to_string()),
                        Some(format!(
                            "当前模式 {mode} 不在允许列表 {:?}，运行时暂停",
                            ALLOWED_GAME_MODES
                        )),
                        Some(json!({
                            "activePlayerName": snapshot.active_player_name,
                            "resolvedChampionName": snapshot.champion_name,
                            "resolvedLevel": snapshot.level,
                            "gameMode": snapshot.game_mode,
                            "gameTime": snapshot.game_time,
                            "sourceEndpoint": snapshot.source_endpoint,
                            "fallbackUsed": snapshot.fallback_used,
                            "allowedModes": ALLOWED_GAME_MODES,
                        })),
                    )
                } else {
                    (
                        Some(LivePlayerSnapshot {
                            champion_name: snapshot.champion_name.clone(),
                            level: snapshot.level,
                        }),
                        live_client_context,
                        None,
                        None,
                        None,
                        Some(json!({
                            "activePlayerName": snapshot.active_player_name,
                            "resolvedChampionName": snapshot.champion_name,
                            "resolvedLevel": snapshot.level,
                            "gameMode": snapshot.game_mode,
                            "gameTime": snapshot.game_time,
                            "sourceEndpoint": snapshot.source_endpoint,
                            "fallbackUsed": snapshot.fallback_used,
                            "allowedModes": ALLOWED_GAME_MODES,
                        })),
                    )
                }
            }
            Err(error) => (
                None,
                RuntimeLiveClientContext::default(),
                Some(match error.pause_reason() {
                    "LiveClientUnavailable" => PauseReason::LiveClientUnavailable,
                    "InvalidLiveClientData" => PauseReason::InvalidLiveClientData,
                    _ => PauseReason::LiveClientUnavailable,
                }),
                Some(error.error_code().to_string()),
                Some(error.to_string()),
                Some(error.diagnostic_payload()),
            ),
        };
        let pending_tiers_before_apply = self.machine.state().pending_tiers.clone();
        self.record_state_machine_input(
            paths,
            trigger_event,
            &live_client_result_category,
            &live_client_context,
            player.as_ref(),
            pause_reason.as_ref(),
            &pending_tiers_before_apply,
            live_client_details,
            start.elapsed().as_millis(),
        )?;

        let input = StateMachineInput {
            player: player.clone(),
            panel_state: self.panel_snapshot.panel_state,
            choices: self.panel_snapshot.choices.clone(),
            selected_slot: self.panel_snapshot.selected_slot,
            pause_reason,
        };
        let state_events = self.machine.apply(input);
        self.last_error_code = error_code.clone();
        let overlay_plan = self.build_overlay_plan();
        self.record_overlay_trigger(
            paths,
            trigger_event,
            &live_client_context,
            &error_code,
            &overlay_plan,
        )?;

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
        )?;
        Ok(overlay_plan)
    }

    fn record_state_machine_input(
        &mut self,
        paths: &AppPaths,
        trigger_event: RuntimeTriggerEvent,
        live_client_result_category: &str,
        live_client_context: &RuntimeLiveClientContext,
        player: Option<&LivePlayerSnapshot>,
        pause_reason: Option<&PauseReason>,
        pending_tiers: &[u8],
        live_client_details: Option<serde_json::Value>,
        duration_ms: u128,
    ) -> Result<(), String> {
        let panel_snapshot = json!({
            "panelState": self.panel_snapshot.panel_state,
            "selectedSlot": self.panel_snapshot.selected_slot,
            "choices": self.panel_snapshot.choices,
        });
        let message = json!({
            "kind": "state-machine-input",
            "triggerEvent": trigger_event,
            "liveClientResultCategory": live_client_result_category,
            "activePlayerName": live_client_context.active_player_name,
            "resolvedChampionName": live_client_context.resolved_champion_name,
            "resolvedLevel": live_client_context.resolved_level,
            "gameMode": live_client_context.game_mode,
            "gameTime": live_client_context.game_time,
            "sourceEndpoint": live_client_context.source_endpoint,
            "fallbackUsed": live_client_context.fallback_used,
            "championName": player.map(|current| current.champion_name.as_str()),
            "level": player.map(|current| current.level),
            "panelSnapshot": panel_snapshot,
            "pendingTiers": pending_tiers,
            "pausedReason": pause_reason.map(|reason| format!("{reason:?}")),
            "liveClientDetails": live_client_details,
        });

        telemetry::write_event(
            paths,
            TelemetryEventInput {
                stage: "runtime-orchestrator".to_string(),
                input_summary: format!(
                    "状态机输入: live_client={}, active_player={:?}, champion={:?}, level={:?}, mode={:?}, source={:?}, fallback_used={:?}, panel={:?}, pending_tiers={:?}, paused_reason={:?}",
                    live_client_result_category,
                    live_client_context.active_player_name,
                    player.map(|current| current.champion_name.as_str()),
                    player.map(|current| current.level),
                    live_client_context.game_mode,
                    live_client_context.source_endpoint,
                    live_client_context.fallback_used,
                    self.panel_snapshot.panel_state,
                    pending_tiers,
                    pause_reason
                ),
                output_summary: "状态机输入摘要已写入".to_string(),
                duration_ms,
                level: if pause_reason.is_some() {
                    "warn".to_string()
                } else {
                    "info".to_string()
                },
                error_code: pause_reason.map(|_| {
                    if matches!(pause_reason, Some(PauseReason::UnsupportedGameMode)) {
                        MODE_MISMATCH_ERROR_CODE.to_string()
                    } else if live_client_result_category == "http_error" {
                        "HEX-LIVE-CLIENT-UNAVAILABLE".to_string()
                    } else {
                        "HEX-LIVE-CLIENT-PAYLOAD".to_string()
                    }
                }),
                message: message.to_string(),
            },
        )?;

        Ok(())
    }

    fn record_overlay_trigger(
        &mut self,
        paths: &AppPaths,
        trigger_event: RuntimeTriggerEvent,
        live_client_context: &RuntimeLiveClientContext,
        error_code: &Option<String>,
        overlay_plan: &RuntimeOverlayPlan,
    ) -> Result<(), String> {
        let state = self.machine.state();
        let has_visible_choices = !state.visible_choices.is_empty();
        let has_pending_tiers = !state.pending_tiers.is_empty();
        let has_panel_choices = !self.panel_snapshot.choices.is_empty();
        let (planned_action, plan_reason, planned_slot_count, is_ready) = match overlay_plan {
            RuntimeOverlayPlan::Show { reason, slots } => {
                ("show", reason.as_str(), slots.len(), true)
            }
            RuntimeOverlayPlan::Hide { reason } => ("hide", reason.as_str(), 0, false),
        };
        let base_message = json!({
            "triggerEvent": trigger_event,
            "activePlayerName": live_client_context.active_player_name,
            "resolvedChampionName": live_client_context.resolved_champion_name,
            "resolvedLevel": live_client_context.resolved_level,
            "gameMode": live_client_context.game_mode,
            "gameTime": live_client_context.game_time,
            "sourceEndpoint": live_client_context.source_endpoint,
            "fallbackUsed": live_client_context.fallback_used,
            "allowedModes": ALLOWED_GAME_MODES,
            "panelState": self.panel_snapshot.panel_state,
            "pendingTiers": state.pending_tiers,
            "pauseReason": state.pause_reason.as_ref().map(|reason| format!("{reason:?}")),
            "hasPendingTiers": has_pending_tiers,
            "hasVisibleChoices": has_visible_choices,
            "hasPendingChoices": has_panel_choices,
            "visibleChoiceCount": state.visible_choices.len(),
            "plannedAction": planned_action,
            "plannedReason": plan_reason,
            "plannedSlotCount": planned_slot_count,
        });
        self.write_runtime_log(
            paths,
            "overlay-trigger-check",
            "overlay 触发检查已记录",
            "info",
            None,
            &base_message,
        )?;

        if is_ready {
            self.write_runtime_log(
                paths,
                "overlay-trigger-ready",
                "overlay 已满足触发条件",
                "info",
                None,
                &base_message,
            )?;
        } else {
            self.write_runtime_log(
                paths,
                "overlay-trigger-skipped",
                "overlay 未满足触发条件",
                if error_code.is_some() { "warn" } else { "info" },
                error_code.clone(),
                &base_message,
            )?;
        }

        Ok(())
    }

    fn build_overlay_plan(&self) -> RuntimeOverlayPlan {
        let state = self.machine.state();
        if let Some(reason) = state.pause_reason.as_ref() {
            return RuntimeOverlayPlan::Hide {
                reason: format!("运行时已暂停：{reason:?}"),
            };
        }
        if self.panel_snapshot.panel_state != PanelState::Expanded {
            return RuntimeOverlayPlan::Hide {
                reason: "海克斯面板未展开".to_string(),
            };
        }
        if state.pending_tiers.is_empty() || state.pending_tier.is_none() {
            return RuntimeOverlayPlan::Hide {
                reason: "当前没有待处理海克斯档位".to_string(),
            };
        }
        if state.visible_choices.is_empty() {
            return RuntimeOverlayPlan::Hide {
                reason: "当前没有可展示的海克斯选项".to_string(),
            };
        }

        let pending_tier = state.pending_tier.unwrap_or_default();
        let champion_name = state
            .player
            .as_ref()
            .map(|player| player.champion_name.as_str())
            .unwrap_or("当前对局");
        RuntimeOverlayPlan::Show {
            reason: format!("海克斯面板已展开且待处理档位 {pending_tier} 可展示"),
            slots: state
                .visible_choices
                .iter()
                .map(|(slot, augment_id)| OverlaySlotData {
                    slot: *slot,
                    title: augment_id.clone(),
                    body: Some(format!(
                        "{champion_name} · 第 {pending_tier} 档海克斯 · 槽位 {slot}"
                    )),
                    augment_id: Some(augment_id.clone()),
                    rank: None,
                    score: None,
                })
                .collect(),
        }
    }

    fn sync_overlay(
        &self,
        app: &AppHandle,
        paths: &AppPaths,
        trigger_event: RuntimeTriggerEvent,
        overlay_plan: &RuntimeOverlayPlan,
    ) {
        let result = match overlay_plan {
            RuntimeOverlayPlan::Show { reason, slots } => {
                overlay::sync_runtime_overlay_inner(app, slots.clone(), reason)
            }
            RuntimeOverlayPlan::Hide { reason } => overlay::hide_runtime_overlay_inner(app, reason),
        };

        match result {
            Ok(report) => self.log_overlay_sync_result(
                paths,
                trigger_event,
                overlay_plan,
                &report,
                None,
            ),
            Err(error) => self.log_overlay_sync_result(
                paths,
                trigger_event,
                overlay_plan,
                &RuntimeOverlaySyncReport {
                    label: "hex-assistant-overlay".to_string(),
                    action: match overlay_plan {
                        RuntimeOverlayPlan::Show { .. } => overlay::RuntimeOverlaySyncAction::Created,
                        RuntimeOverlayPlan::Hide { .. } => overlay::RuntimeOverlaySyncAction::Hidden,
                    },
                    visible: matches!(overlay_plan, RuntimeOverlayPlan::Show { .. }),
                    window_exists: false,
                    slot_count: match overlay_plan {
                        RuntimeOverlayPlan::Show { slots, .. } => slots.len(),
                        RuntimeOverlayPlan::Hide { .. } => 0,
                    },
                    reason: match overlay_plan {
                        RuntimeOverlayPlan::Show { reason, .. } => reason.clone(),
                        RuntimeOverlayPlan::Hide { reason } => reason.clone(),
                    },
                    log_path: None,
                    message: "Overlay 自动同步失败".to_string(),
                },
                Some(error.to_string()),
            ),
        }
    }

    fn log_overlay_sync_result(
        &self,
        paths: &AppPaths,
        trigger_event: RuntimeTriggerEvent,
        overlay_plan: &RuntimeOverlayPlan,
        report: &RuntimeOverlaySyncReport,
        error_message: Option<String>,
    ) {
        let payload = json!({
            "triggerEvent": trigger_event,
            "plan": overlay_plan,
            "report": report,
            "state": self.machine.state(),
            "panelState": self.panel_snapshot.panel_state,
            "errorMessage": error_message,
        });
        let kind = if error_message.is_some() {
            "overlay-sync-failed"
        } else {
            "overlay-sync-applied"
        };
        let output_summary = if error_message.is_some() {
            "overlay 自动同步失败"
        } else {
            "overlay 自动同步完成"
        };
        let level = if error_message.is_some() { "warn" } else { "info" };
        let error_code = error_message
            .as_ref()
            .map(|_| "HEX-OVERLAY-SYNC-FAILED".to_string());
        if let Err(log_error) =
            self.write_runtime_log(paths, kind, output_summary, level, error_code, &payload)
        {
            eprintln!("写入 overlay 自动同步日志失败: {log_error}");
        }
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

        let log_payload = json!({
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

    fn write_runtime_log(
        &self,
        paths: &AppPaths,
        kind: &str,
        output_summary: &str,
        level: &str,
        error_code: Option<String>,
        payload: &serde_json::Value,
    ) -> Result<(), String> {
        telemetry::write_event(
            paths,
            TelemetryEventInput {
                stage: "runtime-orchestrator".to_string(),
                input_summary: kind.to_string(),
                output_summary: output_summary.to_string(),
                duration_ms: 0,
                level: level.to_string(),
                error_code,
                message: json!({
                    "kind": kind,
                    "payload": payload,
                })
                .to_string(),
            },
        )?;
        Ok(())
    }
}

impl Default for RuntimeLiveClientContext {
    fn default() -> Self {
        Self {
            active_player_name: None,
            resolved_champion_name: None,
            resolved_level: None,
            game_mode: None,
            game_time: None,
            source_endpoint: None,
            fallback_used: None,
        }
    }
}

impl RuntimeLiveClientContext {
    fn from_snapshot(snapshot: &ResolvedPlayerSnapshot) -> Self {
        Self {
            active_player_name: Some(snapshot.active_player_name.clone()),
            resolved_champion_name: Some(snapshot.champion_name.clone()),
            resolved_level: Some(snapshot.level),
            game_mode: snapshot.game_mode.clone(),
            game_time: snapshot.game_time,
            source_endpoint: Some(snapshot.source_endpoint.clone()),
            fallback_used: Some(snapshot.fallback_used),
        }
    }
}

fn is_allowed_game_mode(game_mode: Option<&str>) -> bool {
    game_mode
        .map(str::trim)
        .filter(|mode| !mode.is_empty())
        .map(|mode| {
            ALLOWED_GAME_MODES
                .iter()
                .any(|allowed_mode| allowed_mode.eq_ignore_ascii_case(mode))
        })
        .unwrap_or(false)
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
                Ok(ResolvedPlayerSnapshot {
                    active_player_name: "loccen#65238".to_string(),
                    champion_name: "Ahri".to_string(),
                    level: 11,
                    game_mode: Some("KIWI".to_string()),
                    game_time: Some(321.0),
                    source_endpoint: "https://127.0.0.1:2999/liveclientdata/allgamedata"
                        .to_string(),
                    fallback_used: false,
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
        let content = std::fs::read_to_string(paths.app_log_path()).expect("应能读取结构化日志");
        assert!(content.contains("state-machine-input"));
        assert!(content.contains("pendingTiers"));
        assert!(content.contains("activePlayerName"));
        assert!(content.contains("overlay-trigger-ready"));

        let _ = std::fs::remove_dir_all(paths.root);
    }

    #[test]
    fn unsupported_mode_pauses_and_records_overlay_skip() {
        let paths = temp_paths("runtime-orchestrator-mode-filter");
        paths.ensure_all().expect("应能创建测试目录");
        let mut orchestrator = RuntimeOrchestrator::new();
        orchestrator.apply_panel_snapshot(Some(CalibratedPanelSnapshot {
            panel_state: PanelState::Collapsed,
            choices: Vec::new(),
            selected_slot: None,
        }));

        orchestrator
            .tick(
                &paths,
                RuntimeTriggerEvent::LowFrequencyPoll,
                Ok(ResolvedPlayerSnapshot {
                    active_player_name: "loccen#65238".to_string(),
                    champion_name: "Ahri".to_string(),
                    level: 5,
                    game_mode: Some("ARAM".to_string()),
                    game_time: Some(90.0),
                    source_endpoint: "https://127.0.0.1:2999/liveclientdata/allgamedata"
                        .to_string(),
                    fallback_used: false,
                }),
            )
            .expect("模式过滤日志应成功");

        let snapshot = orchestrator.snapshot(false);
        assert_eq!(
            snapshot.state.status,
            crate::state_machine::AssistantStatus::Paused
        );
        assert_eq!(
            snapshot.state.pause_reason,
            Some(PauseReason::UnsupportedGameMode)
        );
        let content = std::fs::read_to_string(paths.app_log_path()).expect("应能读取结构化日志");
        assert!(content.contains("HEX-LIVE-CLIENT-MODE-MISMATCH"));
        assert!(content.contains("overlay-trigger-skipped"));
        assert!(content.contains("allowedModes"));

        let _ = std::fs::remove_dir_all(paths.root);
    }

    #[test]
    fn overlay_plan_shows_visible_choices_when_runtime_ready() {
        let mut orchestrator = RuntimeOrchestrator::new();
        orchestrator.machine.apply(StateMachineInput {
            player: Some(LivePlayerSnapshot {
                champion_name: "Ahri".to_string(),
                level: 7,
            }),
            panel_state: PanelState::Expanded,
            choices: vec![
                AugmentChoice {
                    slot: 1,
                    augment_id: "prismatic-ticket".to_string(),
                },
                AugmentChoice {
                    slot: 3,
                    augment_id: "trade-sector".to_string(),
                },
            ],
            selected_slot: None,
            pause_reason: None,
        });
        orchestrator.panel_snapshot = CalibratedPanelSnapshot {
            panel_state: PanelState::Expanded,
            choices: vec![
                AugmentChoice {
                    slot: 1,
                    augment_id: "prismatic-ticket".to_string(),
                },
                AugmentChoice {
                    slot: 3,
                    augment_id: "trade-sector".to_string(),
                },
            ],
            selected_slot: None,
        };

        let plan = orchestrator.build_overlay_plan();
        match plan {
            RuntimeOverlayPlan::Show { reason, slots } => {
                assert!(reason.contains("待处理档位 3"));
                assert_eq!(slots.len(), 2);
                assert_eq!(slots[0].slot, 1);
                assert_eq!(slots[0].title, "prismatic-ticket");
                assert_eq!(slots[0].augment_id.as_deref(), Some("prismatic-ticket"));
                assert!(slots[0]
                    .body
                    .as_deref()
                    .is_some_and(|body| body.contains("Ahri")));
            }
            RuntimeOverlayPlan::Hide { reason } => {
                panic!("期望生成显示计划，实际隐藏: {reason}");
            }
        }
    }

    #[test]
    fn overlay_plan_hides_when_panel_is_collapsed() {
        let mut orchestrator = RuntimeOrchestrator::new();
        orchestrator.machine.apply(StateMachineInput {
            player: Some(LivePlayerSnapshot {
                champion_name: "Ahri".to_string(),
                level: 7,
            }),
            panel_state: PanelState::Collapsed,
            choices: vec![AugmentChoice {
                slot: 1,
                augment_id: "prismatic-ticket".to_string(),
            }],
            selected_slot: None,
            pause_reason: None,
        });
        orchestrator.panel_snapshot = CalibratedPanelSnapshot {
            panel_state: PanelState::Collapsed,
            choices: vec![AugmentChoice {
                slot: 1,
                augment_id: "prismatic-ticket".to_string(),
            }],
            selected_slot: None,
        };

        let plan = orchestrator.build_overlay_plan();
        assert_eq!(
            plan,
            RuntimeOverlayPlan::Hide {
                reason: "海克斯面板未展开".to_string(),
            }
        );
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
