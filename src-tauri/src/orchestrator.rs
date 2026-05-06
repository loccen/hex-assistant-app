use crate::apex;
use crate::app_paths::AppPaths;
use crate::live_client::{LiveClientDataApi, LiveClientError, ResolvedPlayerSnapshot};
use crate::models::TelemetryEventInput;
use crate::overlay::{self, OverlaySlotData, RuntimeOverlaySyncReport};
use crate::runtime_panel;
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
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tauri::AppHandle;

const RECENT_EVENT_LIMIT: usize = 40;
const MIN_ACTIVE_LISTEN_INTERVAL_MS: u64 = 250;
const MIN_IDLE_LISTEN_INTERVAL_MS: u64 = 1_500;
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
    listener_generation: Arc<AtomicU64>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl Default for RuntimeOrchestratorHandle {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(RuntimeOrchestrator::new())),
            listening: Arc::new(AtomicBool::new(false)),
            listener_generation: Arc::new(AtomicU64::new(0)),
            worker: Mutex::new(None),
        }
    }
}

impl RuntimeOrchestratorHandle {
    fn apply_listener_request(&self, request: RuntimeTriggerRequest) -> Result<(), String> {
        let mut orchestrator = self
            .inner
            .lock()
            .map_err(|_| "运行时编排器状态锁已损坏".to_string())?;
        orchestrator.apply_panel_snapshot(request.panel_snapshot);
        Ok(())
    }

    fn record_listener_control_event(
        &self,
        paths: &AppPaths,
        trigger_event: RuntimeTriggerEvent,
    ) -> Result<(), String> {
        let mut orchestrator = self
            .inner
            .lock()
            .map_err(|_| "运行时编排器状态锁已损坏".to_string())?;
        orchestrator.record_control_event(paths, trigger_event)
    }

    fn register_listener_start(
        &self,
        paths: &AppPaths,
        request: RuntimeTriggerRequest,
    ) -> Result<Option<u64>, String> {
        self.apply_listener_request(request)?;

        if self.listening.swap(true, Ordering::SeqCst) {
            return Ok(None);
        }

        let generation = self.listener_generation.fetch_add(1, Ordering::SeqCst) + 1;
        if let Err(error) =
            self.record_listener_control_event(paths, RuntimeTriggerEvent::ListenerStarted)
        {
            self.listening.store(false, Ordering::SeqCst);
            self.listener_generation.fetch_add(1, Ordering::SeqCst);
            return Err(error);
        }

        Ok(Some(generation))
    }

    fn install_worker_handle(
        &self,
        generation: u64,
        handle: JoinHandle<()>,
    ) -> Result<bool, String> {
        let should_install = self.listening.load(Ordering::SeqCst)
            && self.listener_generation.load(Ordering::SeqCst) == generation;
        if !should_install {
            handle
                .join()
                .map_err(|_| "运行时监听线程退出失败".to_string())?;
            return Ok(false);
        }

        let mut worker_slot = self
            .worker
            .lock()
            .map_err(|_| "运行时监听线程锁已损坏".to_string())?;
        let should_install = self.listening.load(Ordering::SeqCst)
            && self.listener_generation.load(Ordering::SeqCst) == generation;
        if !should_install {
            drop(worker_slot);
            handle
                .join()
                .map_err(|_| "运行时监听线程退出失败".to_string())?;
            return Ok(false);
        }

        *worker_slot = Some(handle);
        Ok(true)
    }

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
        let live_result = LiveClientDataApi::new().fetch_resolved_player_snapshot();
        let (overlay_plan, snapshot) = {
            let mut orchestrator = self
                .inner
                .lock()
                .map_err(|_| "运行时编排器状态锁已损坏".to_string())?;
            orchestrator.apply_panel_snapshot(request.panel_snapshot);
            let overlay_plan =
                orchestrator.tick(Some(app), &paths, RuntimeTriggerEvent::Manual, live_result)?;
            let snapshot = orchestrator.snapshot(self.listening.load(Ordering::SeqCst));
            (overlay_plan, snapshot)
        };
        self.sync_overlay_without_lock(app, &paths, RuntimeTriggerEvent::Manual, &overlay_plan);
        Ok(snapshot)
    }

    pub fn start_listener(
        &self,
        app: &AppHandle,
        request: RuntimeTriggerRequest,
    ) -> Result<RuntimeLoopSnapshot, String> {
        let paths = AppPaths::from_app(app)?;
        paths.ensure_all()?;
        let settings = load_or_create_settings(&paths)?;
        let Some(generation) = self.register_listener_start(&paths, request)? else {
            return self.snapshot();
        };

        let inner = Arc::clone(&self.inner);
        let listening = Arc::clone(&self.listening);
        let listener_generation = Arc::clone(&self.listener_generation);
        let worker_paths = paths.clone();
        let worker_app = app.clone();
        let worker_settings = settings.clone();
        let handle = thread::spawn(move || {
            while listening.load(Ordering::SeqCst)
                && listener_generation.load(Ordering::SeqCst) == generation
            {
                let live_result = LiveClientDataApi::new().fetch_resolved_player_snapshot();
                let (overlay_plan, next_interval_ms) = if let Ok(mut orchestrator) = inner.lock() {
                    let result = orchestrator.tick(
                        Some(&worker_app),
                        &worker_paths,
                        RuntimeTriggerEvent::LowFrequencyPoll,
                        live_result,
                    );
                    orchestrator.log_poll_interval_decision(&worker_paths, &worker_settings);
                    (
                        result.ok(),
                        orchestrator.next_poll_interval_ms(&worker_settings),
                    )
                } else {
                    (None, worker_settings.capture.idle_poll_interval_ms)
                };
                if let Some(overlay_plan) = overlay_plan {
                    let result = apply_overlay_plan(&worker_app, &overlay_plan);
                    if let Ok(orchestrator) = inner.lock() {
                        match result {
                            Ok(report) => orchestrator.log_overlay_sync_result(
                                &worker_paths,
                                RuntimeTriggerEvent::LowFrequencyPoll,
                                &overlay_plan,
                                &report,
                                None,
                            ),
                            Err(error) => orchestrator.log_overlay_sync_result(
                                &worker_paths,
                                RuntimeTriggerEvent::LowFrequencyPoll,
                                &overlay_plan,
                                &RuntimeOverlaySyncReport {
                                    label: "hex-assistant-overlay".to_string(),
                                    action: match overlay_plan {
                                        RuntimeOverlayPlan::Show { .. } => {
                                            overlay::RuntimeOverlaySyncAction::Created
                                        }
                                        RuntimeOverlayPlan::Hide { .. } => {
                                            overlay::RuntimeOverlaySyncAction::Hidden
                                        }
                                    },
                                    visible: matches!(
                                        overlay_plan,
                                        RuntimeOverlayPlan::Show { .. }
                                    ),
                                    window_exists: false,
                                    slot_count: match &overlay_plan {
                                        RuntimeOverlayPlan::Show { slots, .. } => slots.len(),
                                        RuntimeOverlayPlan::Hide { .. } => 0,
                                    },
                                    reason: match &overlay_plan {
                                        RuntimeOverlayPlan::Show { reason, .. } => reason.clone(),
                                        RuntimeOverlayPlan::Hide { reason } => reason.clone(),
                                    },
                                    log_path: None,
                                    message: "Overlay 自动同步失败".to_string(),
                                },
                                Some(error),
                            ),
                        }
                    }
                }
                if !listening.load(Ordering::SeqCst)
                    || listener_generation.load(Ordering::SeqCst) != generation
                {
                    break;
                }
                thread::sleep(Duration::from_millis(next_interval_ms));
            }
        });

        self.install_worker_handle(generation, handle)?;

        self.snapshot()
    }

    pub fn stop_listener(&self, app: &AppHandle) -> Result<RuntimeLoopSnapshot, String> {
        let was_listening = self.listening.swap(false, Ordering::SeqCst);
        self.listener_generation.fetch_add(1, Ordering::SeqCst);
        let worker_handle = self
            .worker
            .lock()
            .map_err(|_| "运行时监听线程锁已损坏".to_string())?
            .take();
        let had_worker = worker_handle.is_some();
        if let Some(handle) = worker_handle {
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
        if was_listening || had_worker {
            orchestrator.record_control_event(&paths, RuntimeTriggerEvent::ListenerStopped)?;
        }
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

    fn sync_overlay_without_lock(
        &self,
        app: &AppHandle,
        paths: &AppPaths,
        trigger_event: RuntimeTriggerEvent,
        overlay_plan: &RuntimeOverlayPlan,
    ) {
        let result = apply_overlay_plan(app, overlay_plan);
        if let Ok(orchestrator) = self.inner.lock() {
            match result {
                Ok(report) => orchestrator.log_overlay_sync_result(
                    paths,
                    trigger_event,
                    overlay_plan,
                    &report,
                    None,
                ),
                Err(error) => orchestrator.log_overlay_sync_result(
                    paths,
                    trigger_event,
                    overlay_plan,
                    &RuntimeOverlaySyncReport {
                        label: "hex-assistant-overlay".to_string(),
                        action: match overlay_plan {
                            RuntimeOverlayPlan::Show { .. } => {
                                overlay::RuntimeOverlaySyncAction::Created
                            }
                            RuntimeOverlayPlan::Hide { .. } => {
                                overlay::RuntimeOverlaySyncAction::Hidden
                            }
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
                    Some(error),
                ),
            }
        }
    }
}

fn apply_overlay_plan(
    app: &AppHandle,
    overlay_plan: &RuntimeOverlayPlan,
) -> Result<RuntimeOverlaySyncReport, String> {
    match overlay_plan {
        RuntimeOverlayPlan::Show { reason, slots } => {
            overlay::sync_runtime_overlay_inner(app, slots.clone(), reason)
                .map_err(|error| error.to_string())
        }
        RuntimeOverlayPlan::Hide { reason } => {
            overlay::hide_runtime_overlay_inner(app, reason).map_err(|error| error.to_string())
        }
    }
}

#[derive(Debug)]
struct RuntimeOrchestrator {
    machine: AssistantStateMachine,
    panel_snapshot: CalibratedPanelSnapshot,
    overlay_slots: Vec<OverlaySlotData>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeOverlayResolvedChoice {
    slot: u8,
    raw_name: String,
    title: String,
    augment_id: Option<String>,
    lookup_name: String,
}

impl RuntimeOrchestrator {
    fn new() -> Self {
        Self {
            machine: AssistantStateMachine::new(),
            panel_snapshot: CalibratedPanelSnapshot::default(),
            overlay_slots: Vec::new(),
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
        app: Option<&AppHandle>,
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
        if let Some(app) = app {
            self.refresh_panel_snapshot(app, paths, player.as_ref(), trigger_event);
        }
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
        self.refresh_overlay_slots(paths);
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

    fn refresh_panel_snapshot(
        &mut self,
        app: &AppHandle,
        paths: &AppPaths,
        player: Option<&LivePlayerSnapshot>,
        trigger_event: RuntimeTriggerEvent,
    ) {
        let should_detect = player.is_some_and(|current| current.level >= 3);
        if !should_detect {
            if self.panel_snapshot.panel_state != PanelState::Collapsed
                || !self.panel_snapshot.choices.is_empty()
            {
                self.panel_snapshot = CalibratedPanelSnapshot::default();
            }
            return;
        }

        let settings = match load_or_create_settings(paths) {
            Ok(settings) => settings,
            Err(error) => {
                self.log_panel_snapshot_refresh(
                    paths,
                    trigger_event,
                    None,
                    Some(format!("读取设置失败: {error}")),
                );
                return;
            }
        };
        match runtime_panel::detect_runtime_panel_snapshot(app, paths, &settings) {
            Ok(snapshot) => {
                let report = snapshot.report.clone();
                self.panel_snapshot = CalibratedPanelSnapshot {
                    panel_state: snapshot.panel_state,
                    choices: snapshot.choices,
                    selected_slot: None,
                };
                self.log_panel_snapshot_refresh(paths, trigger_event, Some(report), None);
            }
            Err(error) => {
                self.log_panel_snapshot_refresh(paths, trigger_event, None, Some(error));
            }
        }
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

    fn refresh_overlay_slots(&mut self, paths: &AppPaths) {
        let state = self.machine.state();
        if state.pause_reason.is_some()
            || self.panel_snapshot.panel_state != PanelState::Expanded
            || state.pending_tiers.is_empty()
            || state.pending_tier.is_none()
            || state.visible_choices.is_empty()
        {
            self.overlay_slots.clear();
            return;
        }

        let Some(player) = state.player.as_ref() else {
            self.overlay_slots.clear();
            return;
        };
        let champion_name = player.champion_name.clone();
        let pending_tier = state.pending_tier.unwrap_or_default();
        let visible_choices = state
            .visible_choices
            .iter()
            .map(|(slot, raw_name)| (*slot, raw_name.clone()))
            .collect::<Vec<_>>();
        let _ = state;

        let settings = load_or_create_settings(paths).unwrap_or_default();
        let dictionary = load_overlay_augment_dictionary(paths, &settings);
        self.overlay_slots = visible_choices
            .iter()
            .map(|(slot, raw_name)| {
                let resolved_choice = resolve_overlay_choice(*slot, raw_name, dictionary.as_ref());
                build_overlay_slot_data(
                    paths,
                    &settings,
                    &champion_name,
                    pending_tier,
                    resolved_choice,
                )
            })
            .collect();
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
        if self.overlay_slots.is_empty() {
            return RuntimeOverlayPlan::Hide {
                reason: "Overlay 推荐数据尚未准备完成".to_string(),
            };
        }

        let pending_tier = state.pending_tier.unwrap_or_default();
        RuntimeOverlayPlan::Show {
            reason: format!("海克斯面板已展开且待处理档位 {pending_tier} 可展示"),
            slots: self.overlay_slots.clone(),
        }
    }

    fn next_poll_interval_ms(&self, settings: &crate::models::AppSettings) -> u64 {
        let active_interval_ms = settings
            .capture
            .poll_interval_ms
            .max(MIN_ACTIVE_LISTEN_INTERVAL_MS);
        let idle_interval_ms = settings
            .capture
            .idle_poll_interval_ms
            .max(MIN_IDLE_LISTEN_INTERVAL_MS);
        let state = self.machine.state();
        let should_use_active_interval = state.pause_reason.is_none()
            && state
                .player
                .as_ref()
                .is_some_and(|player| player.level >= 3)
            && (self.panel_snapshot.panel_state == PanelState::Expanded
                || !state.pending_tiers.is_empty()
                || !state.visible_choices.is_empty());

        if should_use_active_interval {
            active_interval_ms
        } else {
            idle_interval_ms
        }
    }

    fn log_panel_snapshot_refresh(
        &self,
        paths: &AppPaths,
        trigger_event: RuntimeTriggerEvent,
        report: Option<runtime_panel::RuntimePanelSnapshotReport>,
        error_message: Option<String>,
    ) {
        let payload = json!({
            "triggerEvent": trigger_event,
            "panelSnapshot": self.panel_snapshot,
            "report": report,
            "errorMessage": error_message,
        });
        let kind = if payload["report"].is_null() {
            "panel-snapshot-refresh-failed"
        } else {
            "panel-snapshot-refresh-applied"
        };
        let output_summary = if payload["report"].is_null() {
            "运行时面板快照刷新失败"
        } else {
            "运行时面板快照刷新完成"
        };
        let level = if error_message.is_some() {
            "warn"
        } else {
            "info"
        };
        let error_code = error_message
            .as_ref()
            .map(|_| "HEX-RUNTIME-PANEL-SNAPSHOT-FAILED".to_string());
        if let Err(log_error) =
            self.write_runtime_log(paths, kind, output_summary, level, error_code, &payload)
        {
            eprintln!("写入运行时面板快照日志失败: {log_error}");
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
            Ok(report) => {
                self.log_overlay_sync_result(paths, trigger_event, overlay_plan, &report, None)
            }
            Err(error) => self.log_overlay_sync_result(
                paths,
                trigger_event,
                overlay_plan,
                &RuntimeOverlaySyncReport {
                    label: "hex-assistant-overlay".to_string(),
                    action: match overlay_plan {
                        RuntimeOverlayPlan::Show { .. } => {
                            overlay::RuntimeOverlaySyncAction::Created
                        }
                        RuntimeOverlayPlan::Hide { .. } => {
                            overlay::RuntimeOverlaySyncAction::Hidden
                        }
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
        let level = if error_message.is_some() {
            "warn"
        } else {
            "info"
        };
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

    fn log_poll_interval_decision(&self, paths: &AppPaths, settings: &crate::models::AppSettings) {
        let state = self.machine.state();
        let next_interval_ms = self.next_poll_interval_ms(settings);
        let active_interval_ms = settings
            .capture
            .poll_interval_ms
            .max(MIN_ACTIVE_LISTEN_INTERVAL_MS);
        let idle_interval_ms = settings
            .capture
            .idle_poll_interval_ms
            .max(MIN_IDLE_LISTEN_INTERVAL_MS);
        let mode = if next_interval_ms == active_interval_ms {
            "active"
        } else {
            "idle"
        };
        let payload = json!({
            "mode": mode,
            "nextIntervalMs": next_interval_ms,
            "activeIntervalMs": active_interval_ms,
            "idleIntervalMs": idle_interval_ms,
            "panelState": self.panel_snapshot.panel_state,
            "pendingTiers": state.pending_tiers,
            "visibleChoiceCount": state.visible_choices.len(),
            "pauseReason": state.pause_reason.as_ref().map(|reason| format!("{reason:?}")),
            "playerLevel": state.player.as_ref().map(|player| player.level),
        });
        if let Err(log_error) = self.write_runtime_log(
            paths,
            "poll-interval-decision",
            "已记录下一轮轮询间隔决策",
            "info",
            None,
            &payload,
        ) {
            eprintln!("写入轮询间隔决策日志失败: {log_error}");
        }
    }
}

fn load_overlay_augment_dictionary(
    paths: &AppPaths,
    settings: &crate::models::AppSettings,
) -> Option<apex::AugmentDictionary> {
    apex::load_augment_dictionary_with_cache(&paths.cache, apex_lookup_settings(settings), false)
        .ok()
        .map(|sync| sync.dictionary)
}

fn apex_lookup_settings(settings: &crate::models::AppSettings) -> apex::ApexLookupSettings {
    apex::ApexLookupSettings {
        cache_ttl_hours: settings.apex_lol.cache_ttl_hours,
        request_timeout_ms: settings.apex_lol.request_timeout_ms,
        failed_cache_ttl_minutes: settings.apex_lol.failed_cache_ttl_minutes,
    }
}

fn resolve_overlay_choice(
    slot: u8,
    raw_name: &str,
    dictionary: Option<&apex::AugmentDictionary>,
) -> RuntimeOverlayResolvedChoice {
    let normalized_raw_name = normalize_overlay_lookup_text(raw_name);
    if let Some(entry) = dictionary.and_then(|dictionary| {
        dictionary.augments.iter().find(|entry| {
            normalize_overlay_lookup_text(&entry.id) == normalized_raw_name
                || normalize_overlay_lookup_text(&entry.name) == normalized_raw_name
                || entry
                    .aliases
                    .iter()
                    .any(|alias| normalize_overlay_lookup_text(alias) == normalized_raw_name)
        })
    }) {
        return RuntimeOverlayResolvedChoice {
            slot,
            raw_name: raw_name.to_string(),
            title: entry.name.clone(),
            augment_id: Some(entry.id.clone()),
            lookup_name: entry.name.clone(),
        };
    }

    RuntimeOverlayResolvedChoice {
        slot,
        raw_name: raw_name.to_string(),
        title: raw_name.to_string(),
        augment_id: None,
        lookup_name: raw_name.to_string(),
    }
}

fn normalize_overlay_lookup_text(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .chars()
        .filter(|ch| !ch.is_whitespace() && *ch != '\'' && *ch != '’')
        .collect()
}

fn build_overlay_slot_data(
    paths: &AppPaths,
    settings: &crate::models::AppSettings,
    champion_name: &str,
    pending_tier: u8,
    choice: RuntimeOverlayResolvedChoice,
) -> OverlaySlotData {
    let fallback_augment_id = choice
        .augment_id
        .clone()
        .or_else(|| Some(choice.raw_name.clone()));
    match apex::lookup_with_cache(
        &paths.cache,
        apex::ApexLookupRequest {
            champion_name: champion_name.to_string(),
            augment_name: choice.lookup_name.clone(),
            force_refresh: false,
        },
        apex_lookup_settings(settings),
    ) {
        Ok(result) if result.status == apex::ApexParseStatus::Ok => {
            let summary = normalize_overlay_summary(&result.summary);
            let tips = build_overlay_tips(result.tip.as_deref(), Some(summary.as_str()));
            let insight = build_success_insight(
                champion_name,
                pending_tier,
                &choice.title,
                result.rating.as_deref(),
            );
            OverlaySlotData {
                slot: choice.slot,
                title: choice.title,
                body: Some(build_overlay_body(&summary, &tips, Some(insight.as_str()))),
                augment_id: fallback_augment_id,
                rank: normalize_overlay_text_option(result.rating.as_deref()),
                score: None,
                summary: Some(summary),
                tips: (!tips.is_empty()).then_some(tips),
                source_label: Some(result.source.clone()),
                source_detail: Some(build_success_source_detail(&result)),
                insight: Some(insight),
            }
        }
        Ok(result) => build_fallback_overlay_slot_data(
            champion_name,
            pending_tier,
            choice,
            fallback_augment_id,
            Some(result.source.clone()),
            build_failure_source_detail(result.status, result.error.as_deref()),
        ),
        Err(error) => build_fallback_overlay_slot_data(
            champion_name,
            pending_tier,
            choice,
            fallback_augment_id,
            Some(apex::APEX_SOURCE_NAME.to_string()),
            format!("查询失败：{}", shrink_error_message(&error)),
        ),
    }
}

fn build_fallback_overlay_slot_data(
    champion_name: &str,
    pending_tier: u8,
    choice: RuntimeOverlayResolvedChoice,
    augment_id: Option<String>,
    source_label: Option<String>,
    source_detail: String,
) -> OverlaySlotData {
    let summary = format!("暂无数据：未找到「{}」的联动推荐", choice.title);
    let tips = vec![format!(
        "{} 第 {} 档请结合阵容、装备和经济节奏手动判断",
        champion_name, pending_tier
    )];
    let insight = format!("{champion_name} · 第 {pending_tier} 档 · 使用兜底展示");
    OverlaySlotData {
        slot: choice.slot,
        title: choice.title,
        body: Some(build_overlay_body(&summary, &tips, Some(insight.as_str()))),
        augment_id,
        rank: None,
        score: None,
        summary: Some(summary),
        tips: Some(tips),
        source_label,
        source_detail: Some(source_detail),
        insight: Some(insight),
    }
}

fn build_overlay_body(summary: &str, tips: &[String], insight: Option<&str>) -> String {
    let mut parts = vec![format!("摘要：{summary}")];
    if !tips.is_empty() {
        parts.push(format!("提醒：{}", tips.join("；")));
    }
    if let Some(insight) = insight.map(str::trim).filter(|insight| !insight.is_empty()) {
        parts.push(format!("结论：{insight}"));
    }
    parts.join(" ")
}

fn build_overlay_tips(tip: Option<&str>, summary: Option<&str>) -> Vec<String> {
    let summary = summary.and_then(normalize_overlay_text);
    normalize_overlay_text_option(tip)
        .filter(|tip| summary.as_ref() != Some(tip))
        .map(|tip| vec![tip])
        .unwrap_or_default()
}

fn build_success_insight(
    champion_name: &str,
    pending_tier: u8,
    augment_name: &str,
    rating: Option<&str>,
) -> String {
    match normalize_overlay_text_option(rating) {
        Some(rating) => format!(
            "{champion_name} 第 {pending_tier} 档选择「{augment_name}」的 ApexLOL 评级为 {rating}"
        ),
        None => format!("{champion_name} 第 {pending_tier} 档可重点关注「{augment_name}」"),
    }
}

fn build_success_source_detail(result: &apex::ApexLookupResult) -> String {
    let mode = if result.cache_hit {
        "缓存命中"
    } else {
        "实时抓取"
    };
    format!("{mode} · {}", result.source_url)
}

fn build_failure_source_detail(status: apex::ApexParseStatus, error: Option<&str>) -> String {
    let reason = match status {
        apex::ApexParseStatus::NoData => "未找到联动数据",
        apex::ApexParseStatus::RequestFailed => "请求失败",
        apex::ApexParseStatus::ParseFailed => "页面解析失败",
        apex::ApexParseStatus::Ok => "已回退到兜底展示",
    };
    match error
        .map(shrink_error_message)
        .filter(|message| !message.is_empty())
    {
        Some(message) => format!("{reason} · {message}"),
        None => reason.to_string(),
    }
}

fn normalize_overlay_summary(summary: &str) -> String {
    normalize_overlay_text(summary).unwrap_or_else(|| apex::NO_DATA_TEXT.to_string())
}

fn normalize_overlay_text(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn normalize_overlay_text_option(value: Option<&str>) -> Option<String> {
    value.and_then(normalize_overlay_text)
}

fn shrink_error_message(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
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
    use chrono::{Duration, Utc};
    use std::collections::BTreeMap;
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
                None,
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
                None,
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
        let paths = temp_paths("runtime-orchestrator-overlay-plan-ready");
        paths.ensure_all().expect("应能创建测试目录");
        seed_augment_dictionary_cache(
            &paths,
            vec![
                apex::AugmentEntry {
                    id: "prismatic-ticket".to_string(),
                    name: "棱彩门票".to_string(),
                    aliases: vec!["Prismatic Ticket".to_string()],
                },
                apex::AugmentEntry {
                    id: "trade-sector".to_string(),
                    name: "交易扇区".to_string(),
                    aliases: vec!["Trade Sector".to_string()],
                },
            ],
        );
        seed_apex_lookup_cache(
            &paths,
            "Ahri",
            "棱彩门票",
            Some("S".to_string()),
            "适合连胜经济局。",
            Some("优先补前排".to_string()),
            apex::ApexParseStatus::Ok,
        );
        seed_apex_lookup_cache(
            &paths,
            "Ahri",
            "交易扇区",
            None,
            apex::NO_DATA_TEXT,
            None,
            apex::ApexParseStatus::NoData,
        );
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
        orchestrator.refresh_overlay_slots(&paths);

        let plan = orchestrator.build_overlay_plan();
        match plan {
            RuntimeOverlayPlan::Show { reason, slots } => {
                assert!(reason.contains("待处理档位 3"));
                assert_eq!(slots.len(), 2);
                assert_eq!(slots[0].slot, 1);
                assert_eq!(slots[0].title, "棱彩门票");
                assert_eq!(slots[0].augment_id.as_deref(), Some("prismatic-ticket"));
                assert_eq!(slots[0].rank.as_deref(), Some("S"));
                assert_eq!(slots[0].summary.as_deref(), Some("适合连胜经济局。"));
                assert_eq!(
                    slots[0].tips.as_ref(),
                    Some(&vec!["优先补前排".to_string()])
                );
                assert_eq!(
                    slots[0].source_label.as_deref(),
                    Some(apex::APEX_SOURCE_NAME)
                );
                assert!(slots[0]
                    .source_detail
                    .as_deref()
                    .is_some_and(|detail| detail.contains("缓存命中")));
                assert!(slots[0]
                    .body
                    .as_deref()
                    .is_some_and(|body| body.contains("结论：Ahri")));
            }
            RuntimeOverlayPlan::Hide { reason } => {
                panic!("期望生成显示计划，实际隐藏: {reason}");
            }
        }

        let _ = std::fs::remove_dir_all(paths.root);
    }

    #[test]
    fn overlay_plan_hides_when_panel_is_collapsed() {
        let mut orchestrator = RuntimeOrchestrator::new();
        orchestrator.overlay_slots = vec![OverlaySlotData {
            slot: 1,
            title: "棱彩门票".to_string(),
            body: Some("摘要：适合连胜经济局。".to_string()),
            augment_id: Some("prismatic-ticket".to_string()),
            rank: Some("S".to_string()),
            score: None,
            summary: Some("适合连胜经济局。".to_string()),
            tips: Some(vec!["优先补前排".to_string()]),
            source_label: Some(apex::APEX_SOURCE_NAME.to_string()),
            source_detail: Some("缓存命中".to_string()),
            insight: Some("Ahri 第 3 档选择「棱彩门票」的 ApexLOL 评级为 S".to_string()),
        }];
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

    #[test]
    fn refresh_overlay_slots_uses_cached_apex_result_and_real_name() {
        let paths = temp_paths("runtime-orchestrator-overlay-cache-hit");
        paths.ensure_all().expect("应能创建测试目录");
        seed_augment_dictionary_cache(
            &paths,
            vec![apex::AugmentEntry {
                id: "prismatic-ticket".to_string(),
                name: "棱彩门票".to_string(),
                aliases: vec!["Prismatic Ticket".to_string()],
            }],
        );
        seed_apex_lookup_cache(
            &paths,
            "Ahri",
            "棱彩门票",
            Some("S".to_string()),
            "适合连胜经济局。",
            Some("优先补前排".to_string()),
            apex::ApexParseStatus::Ok,
        );

        let mut orchestrator = RuntimeOrchestrator::new();
        orchestrator.machine.apply(StateMachineInput {
            player: Some(LivePlayerSnapshot {
                champion_name: "Ahri".to_string(),
                level: 7,
            }),
            panel_state: PanelState::Expanded,
            choices: vec![AugmentChoice {
                slot: 1,
                augment_id: "prismatic-ticket".to_string(),
            }],
            selected_slot: None,
            pause_reason: None,
        });
        orchestrator.panel_snapshot = CalibratedPanelSnapshot {
            panel_state: PanelState::Expanded,
            choices: vec![AugmentChoice {
                slot: 1,
                augment_id: "prismatic-ticket".to_string(),
            }],
            selected_slot: None,
        };

        orchestrator.refresh_overlay_slots(&paths);

        assert_eq!(orchestrator.overlay_slots.len(), 1);
        let slot = &orchestrator.overlay_slots[0];
        assert_eq!(slot.title, "棱彩门票");
        assert_eq!(slot.rank.as_deref(), Some("S"));
        assert_eq!(slot.augment_id.as_deref(), Some("prismatic-ticket"));
        assert_eq!(slot.summary.as_deref(), Some("适合连胜经济局。"));
        assert_eq!(slot.source_label.as_deref(), Some(apex::APEX_SOURCE_NAME));
        assert!(slot
            .source_detail
            .as_deref()
            .is_some_and(|detail| detail.contains("缓存命中")));
        assert_eq!(slot.tips.as_ref(), Some(&vec!["优先补前排".to_string()]));
        assert!(slot
            .body
            .as_deref()
            .is_some_and(|body| body.contains("提醒：优先补前排")));
        assert!(slot
            .insight
            .as_deref()
            .is_some_and(|insight| insight.contains("ApexLOL 评级为 S")));

        let _ = std::fs::remove_dir_all(paths.root);
    }

    #[test]
    fn refresh_overlay_slots_falls_back_to_no_data_without_faking_rank() {
        let paths = temp_paths("runtime-orchestrator-overlay-no-data");
        paths.ensure_all().expect("应能创建测试目录");
        seed_apex_lookup_cache(
            &paths,
            "Ahri",
            "棱彩门票",
            None,
            apex::NO_DATA_TEXT,
            None,
            apex::ApexParseStatus::NoData,
        );

        let mut orchestrator = RuntimeOrchestrator::new();
        orchestrator.machine.apply(StateMachineInput {
            player: Some(LivePlayerSnapshot {
                champion_name: "Ahri".to_string(),
                level: 7,
            }),
            panel_state: PanelState::Expanded,
            choices: vec![AugmentChoice {
                slot: 2,
                augment_id: "棱彩门票".to_string(),
            }],
            selected_slot: None,
            pause_reason: None,
        });
        orchestrator.panel_snapshot = CalibratedPanelSnapshot {
            panel_state: PanelState::Expanded,
            choices: vec![AugmentChoice {
                slot: 2,
                augment_id: "棱彩门票".to_string(),
            }],
            selected_slot: None,
        };

        orchestrator.refresh_overlay_slots(&paths);

        assert_eq!(orchestrator.overlay_slots.len(), 1);
        let slot = &orchestrator.overlay_slots[0];
        assert_eq!(slot.title, "棱彩门票");
        assert!(slot.rank.is_none());
        assert!(slot.score.is_none());
        assert!(slot
            .summary
            .as_deref()
            .is_some_and(|summary| summary.contains("暂无")));
        assert_eq!(slot.source_label.as_deref(), Some(apex::APEX_SOURCE_NAME));
        assert!(slot
            .source_detail
            .as_deref()
            .is_some_and(|detail| detail.contains("未找到联动数据")));
        assert!(slot
            .tips
            .as_ref()
            .is_some_and(|tips| tips.iter().any(|tip| tip.contains("手动判断"))));
        assert!(slot
            .insight
            .as_deref()
            .is_some_and(|insight| insight.contains("兜底展示")));
        assert!(slot
            .body
            .as_deref()
            .is_some_and(|body| body.contains("暂无")));

        let _ = std::fs::remove_dir_all(paths.root);
    }

    #[test]
    fn refresh_overlay_slots_marks_failed_lookup_as_fallback() {
        let paths = temp_paths("runtime-orchestrator-overlay-request-failed");
        paths.ensure_all().expect("应能创建测试目录");
        seed_apex_lookup_cache(
            &paths,
            "Ahri",
            "交易扇区",
            None,
            apex::NO_DATA_TEXT,
            None,
            apex::ApexParseStatus::RequestFailed,
        );

        let mut orchestrator = RuntimeOrchestrator::new();
        orchestrator.machine.apply(StateMachineInput {
            player: Some(LivePlayerSnapshot {
                champion_name: "Ahri".to_string(),
                level: 7,
            }),
            panel_state: PanelState::Expanded,
            choices: vec![AugmentChoice {
                slot: 3,
                augment_id: "交易扇区".to_string(),
            }],
            selected_slot: None,
            pause_reason: None,
        });
        orchestrator.panel_snapshot = CalibratedPanelSnapshot {
            panel_state: PanelState::Expanded,
            choices: vec![AugmentChoice {
                slot: 3,
                augment_id: "交易扇区".to_string(),
            }],
            selected_slot: None,
        };

        orchestrator.refresh_overlay_slots(&paths);

        assert_eq!(orchestrator.overlay_slots.len(), 1);
        let slot = &orchestrator.overlay_slots[0];
        assert!(slot
            .summary
            .as_deref()
            .is_some_and(|summary| summary.contains("暂无")));
        assert_eq!(slot.source_label.as_deref(), Some(apex::APEX_SOURCE_NAME));
        assert!(slot
            .source_detail
            .as_deref()
            .is_some_and(|detail| detail.contains("请求失败")));
        assert!(slot
            .tips
            .as_ref()
            .is_some_and(|tips| tips.iter().any(|tip| tip.contains("手动判断"))));
        assert!(slot
            .body
            .as_deref()
            .is_some_and(|body| body.contains("手动判断") && body.contains("兜底展示")));

        let _ = std::fs::remove_dir_all(paths.root);
    }

    #[test]
    fn register_listener_start_is_idempotent() {
        let paths = temp_paths("runtime-orchestrator-listener-start");
        paths.ensure_all().expect("应能创建测试目录");
        let handle = RuntimeOrchestratorHandle::default();

        let first_generation = handle
            .register_listener_start(
                &paths,
                RuntimeTriggerRequest {
                    panel_snapshot: Some(CalibratedPanelSnapshot::default()),
                },
            )
            .expect("首次注册监听应成功");
        let second_generation = handle
            .register_listener_start(
                &paths,
                RuntimeTriggerRequest {
                    panel_snapshot: Some(CalibratedPanelSnapshot::default()),
                },
            )
            .expect("重复注册监听应返回当前快照");

        assert_eq!(first_generation, Some(1));
        assert_eq!(second_generation, None);
        assert!(handle.listening.load(Ordering::SeqCst));
        assert_eq!(handle.listener_generation.load(Ordering::SeqCst), 1);
        let snapshot = handle.snapshot().expect("应能读取运行时快照");
        assert_eq!(snapshot.recent_events.len(), 1);
        assert_eq!(
            snapshot.recent_events[0].trigger_event,
            RuntimeTriggerEvent::ListenerStarted
        );

        handle.listening.store(false, Ordering::SeqCst);
        handle.listener_generation.fetch_add(1, Ordering::SeqCst);
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

    fn seed_augment_dictionary_cache(paths: &AppPaths, augments: Vec<apex::AugmentEntry>) {
        let fetched_at = Utc::now();
        let cache = apex::ApexAugmentDictionaryCache {
            version: 1,
            source: apex::APEX_SOURCE_NAME.to_string(),
            source_urls: vec!["https://apexlol.info/zh/hextech/".to_string()],
            fetched_at,
            expires_at: fetched_at + Duration::hours(1),
            dictionary: apex::AugmentDictionary {
                locale: "zh-CN".to_string(),
                version: 1,
                augments,
            },
        };
        let path = paths.cache.join("apex-augment-dictionary.zh-CN.json");
        let content = serde_json::to_string_pretty(&cache).expect("词库缓存应可序列化");
        std::fs::write(&path, format!("{content}\n")).expect("应能写入词库缓存");
    }

    fn seed_apex_lookup_cache(
        paths: &AppPaths,
        champion_name: &str,
        augment_name: &str,
        rating: Option<String>,
        summary: &str,
        tip: Option<String>,
        status: apex::ApexParseStatus,
    ) {
        let fetched_at = Utc::now();
        let entry = apex::ApexCacheEntry {
            champion_name: champion_name.to_string(),
            augment_name: augment_name.to_string(),
            rating,
            summary: summary.to_string(),
            tip,
            source: apex::APEX_SOURCE_NAME.to_string(),
            source_url: "https://apexlol.info/zh/hextech/77".to_string(),
            fetched_at,
            expires_at: fetched_at + Duration::hours(1),
            cache_hit: false,
            status,
            error: None,
            request_url: "https://apexlol.info/zh/hextech/77".to_string(),
            duration_ms: 12,
        };
        let cache_path = paths.cache.join("apex-cache").join("cache.json");
        let mut entries = if cache_path.exists() {
            let content = std::fs::read_to_string(&cache_path).expect("应能读取已有 Apex 缓存");
            serde_json::from_str::<apex::ApexCacheFile>(&content)
                .expect("已有 Apex 缓存应可解析")
                .entries
        } else {
            BTreeMap::new()
        };
        entries.insert(apex::lookup_cache_key(champion_name, augment_name), entry);
        let cache = apex::ApexCacheFile {
            version: 1,
            entries,
        };
        let content = serde_json::to_string_pretty(&cache).expect("Apex 缓存应可序列化");
        if let Some(parent) = cache_path.parent() {
            std::fs::create_dir_all(parent).expect("应能创建 Apex 缓存目录");
        }
        std::fs::write(&cache_path, format!("{content}\n")).expect("应能写入 Apex 缓存");
    }
}
