import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import "./App.css";

declare global {
  interface Window {
    __HEX_OVERLAY_BOOTSTRAP__?: OverlayPagePayload;
  }
}

type MonitorDiagnostic = {
  id: number;
  name: string;
  friendlyName: string;
  x: number;
  y: number;
  width: number;
  height: number;
  primary: boolean;
};

type CaptureSampleReport = {
  capturedAt: string;
  monitor: MonitorDiagnostic;
  image: {
    width: number;
    height: number;
    blackScreen: boolean;
    staleFrame: boolean;
  };
  pngPath: string;
};

type ScreenshotDataUrl = {
  path: string;
  dataUrl: string;
  bytes: number;
};

type ScreenshotSize = {
  width: number;
  height: number;
};

type PixelRect = {
  x: number;
  y: number;
  width: number;
  height: number;
};

type PixelPoint = {
  x: number;
  y: number;
};

type CalibrationInput = {
  screenshotSize: ScreenshotSize;
  nameRegions: [PixelRect, PixelRect, PixelRect];
  bottomAnchors: [PixelPoint, PixelPoint, PixelPoint];
  bottomButtonRegion: PixelRect;
};

type CalibrationProfileResult = {
  path: string;
  echo: {
    screenshotSize: ScreenshotSize;
    nameRegionPixels: [PixelRect, PixelRect, PixelRect];
    bottomAnchorPixels: [PixelPoint, PixelPoint, PixelPoint];
    bottomButtonRegionPixels: PixelRect;
  };
};

type RuntimeLoopSnapshot = {
  listening: boolean;
  state: {
    status: string;
    pendingTiers: number[];
    pauseReason?: string | null;
  };
  lastErrorCode?: string | null;
};

type RuntimeTriggerRequest = {
  panelSnapshot: {
    panelState: "expanded" | "collapsed";
    choices: Array<{ slot: number; augmentId: string }>;
    selectedSlot: number | null;
  };
};

type CalibratedNameOcrReport = {
  slots: Array<{
    slot: "left" | "center" | "right";
    rawText: string;
    confidence: number;
    matchScore: number;
    finalName?: string | null;
    failureReason?: string | null;
  }>;
};

type NameRegionOcrPrecheckReport = {
  rawText: string;
  confidence: number;
  matchScore: number;
  finalName?: string | null;
  failureReason?: string | null;
  belowConfidenceThreshold: boolean;
  belowMatchThreshold: boolean;
  requiresUserAdjustment: boolean;
};

type TrayExportEvent = {
  status: "started" | "completed" | "failed";
  zipPath?: string | null;
  includedFiles?: number | null;
  message: string;
};

type OverlaySlotData = {
  slot: number;
  title: string;
  body?: string | null;
  augmentId?: string | null;
  rank?: string | null;
  score?: string | null;
  summary?: string | null;
  tips?: string[] | null;
  sourceLabel?: string | null;
  sourceDetail?: string | null;
  insight?: string | null;
};

type OverlayCardInfo = OverlaySlotData & {
  body: string;
  bounds: PixelRect;
  source: string;
};

type OverlayPagePayload = {
  generatedAt: string;
  mode: string;
  cards: OverlayCardInfo[];
};

type OverlayCardViewModel = {
  slotLabel: string;
  title: string;
  summary: string;
  detail: string | null;
  insight: string | null;
  tips: string[];
  sourceText: string;
  sourceDetail: string | null;
  statusText: string;
  updateText: string;
  scoreText: string | null;
  augmentText: string | null;
  rankText: string | null;
  rankBadgeText: string;
  tone: "fallback" | "neutral" | "strong";
};

type RegionKey =
  | "name-0"
  | "name-1"
  | "name-2"
  | "anchor-0"
  | "anchor-1"
  | "anchor-2"
  | "button";

type RegionDefinition = {
  key: RegionKey;
  label: string;
  help: string;
  kind: "rect" | "point";
};

type MarkState = {
  nameRegions: [PixelRect | null, PixelRect | null, PixelRect | null];
  bottomAnchors: [PixelPoint | null, PixelPoint | null, PixelPoint | null];
  bottomButtonRegion: PixelRect | null;
};

type DragState = {
  key: RegionKey;
  startX: number;
  startY: number;
  currentX: number;
  currentY: number;
};

type Toast = {
  tone: "info" | "success" | "error";
  message: string;
};

type AutomationStatus = {
  phase: "idle" | "starting" | "active" | "stopping" | "paused" | "error";
  message: string;
};

type CalibrationStepId = "prepare-lobby" | "set-display" | "check-layout" | "capture";

type CalibrationStep = {
  id: CalibrationStepId;
  title: string;
  action: string;
  detail: string;
};

type NameOcrPreviewSlot = {
  slot: "left" | "center" | "right";
  text: string | null;
  rawText: string | null;
  confidence: number | null;
  matchScore: number | null;
  lowConfidence: boolean;
  hint: string | null;
  source: "placeholder" | "live" | "ocr-check";
  status: "idle" | "pending" | "ready" | "error";
};

const regionDefinitions: RegionDefinition[] = [
  { key: "name-0", label: "左侧符文名称", help: "拖拽框住左侧名称文字", kind: "rect" },
  { key: "name-1", label: "中间符文名称", help: "拖拽框住中间名称文字", kind: "rect" },
  { key: "name-2", label: "右侧符文名称", help: "拖拽框住右侧名称文字", kind: "rect" },
  { key: "anchor-0", label: "左侧卡片底部", help: "点击左侧卡片底部中心", kind: "point" },
  { key: "anchor-1", label: "中间卡片底部", help: "点击中间卡片底部中心", kind: "point" },
  { key: "anchor-2", label: "右侧卡片底部", help: "点击右侧卡片底部中心", kind: "point" },
  { key: "button", label: "展开/隐藏按钮", help: "拖拽框住底部按钮区域", kind: "rect" },
];

const slotText: Record<"left" | "center" | "right", string> = {
  left: "左侧",
  center: "中间",
  right: "右侧",
};

const runtimeStatusText: Record<string, string> = {
  waitingForGame: "等待对局",
  waitingForTier: "等待海克斯阶段",
  pendingSelection: "识别待确认",
  paused: "已暂停",
};

const runtimeStatusAccent: Record<string, string> = {
  waitingForGame: "neutral",
  waitingForTier: "watch",
  pendingSelection: "live",
  paused: "warn",
};

const calibrationSteps: CalibrationStep[] = [
  { id: "prepare-lobby", title: "步骤 1", action: "进入一局海克斯乱斗自定义。", detail: "停在海克斯三选一画面。" },
  { id: "set-display", title: "步骤 2", action: "切到无边框。", detail: "保持这次分辨率和缩放不再变化。" },
  { id: "check-layout", title: "步骤 3", action: "确认 UI 已完全展开。", detail: "三张海克斯卡和底部按钮都要看得见。" },
  { id: "capture", title: "步骤 4", action: "选显示器并截图。", detail: "截图完成后直接进入标记工作台。" },
];

function emptyMarkState(): MarkState {
  return {
    nameRegions: [null, null, null],
    bottomAnchors: [null, null, null],
    bottomButtonRegion: null,
  };
}

function App() {
  const isOverlayView = new URLSearchParams(window.location.search).get("view") === "overlay";
  return isOverlayView ? <OverlayPage /> : <PlayerApp />;
}

function PlayerApp() {
  const [calibrated, setCalibrated] = useState<boolean | null>(null);
  const [profile, setProfile] = useState<CalibrationProfileResult | null>(null);
  const [runtime, setRuntime] = useState<RuntimeLoopSnapshot | null>(null);
  const [automationStatus, setAutomationStatus] = useState<AutomationStatus>({
    phase: "idle",
    message: "正在检查校准和监听状态。",
  });
  const [monitors, setMonitors] = useState<MonitorDiagnostic[]>([]);
  const [selectedMonitorId, setSelectedMonitorId] = useState("");
  const [mode, setMode] = useState<"status" | "calibration">("status");
  const [capture, setCapture] = useState<CaptureSampleReport | null>(null);
  const [screenshot, setScreenshot] = useState<ScreenshotDataUrl | null>(null);
  const [marks, setMarks] = useState<MarkState>(() => emptyMarkState());
  const [activeRegion, setActiveRegion] = useState<RegionKey>("name-0");
  const [calibrationStepIndex, setCalibrationStepIndex] = useState(0);
  const [drag, setDrag] = useState<DragState | null>(null);
  const [countdown, setCountdown] = useState<number | null>(null);
  const [ocrReport, setOcrReport] = useState<CalibratedNameOcrReport | null>(null);
  const [liveNameOcr, setLiveNameOcr] = useState<NameOcrPreviewSlot[]>(() => buildEmptyNameOcrPreview());
  const [toast, setToast] = useState<Toast | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const runtimeRef = useRef<RuntimeLoopSnapshot | null>(null);
  const controlQueueRef = useRef<Promise<void>>(Promise.resolve());
  const nameOcrSlotKeysRef = useRef<[string | null, string | null, string | null]>([null, null, null]);
  const nameOcrRequestIdsRef = useRef<[number, number, number]>([0, 0, 0]);
  const lastCalibrationSnapshotRef = useRef<string | null>(null);

  useEffect(() => {
    runtimeRef.current = runtime;
  }, [runtime]);

  useEffect(() => {
    void boot();

    const unlistenRecalibrate = listen("hex-assistant://recalibrate", () => {
      void requestRecalibration("监听已暂停，请重新完成校准。");
    });
    const unlistenExport = listen<TrayExportEvent>("hex-assistant://export-status", (event) => {
      setToast({
        tone: event.payload.status === "failed" ? "error" : event.payload.status === "completed" ? "success" : "info",
        message: event.payload.message,
      });
    });

    return () => {
      void unlistenRecalibrate.then((dispose) => dispose());
      void unlistenExport.then((dispose) => dispose());
    };
  }, []);

  useEffect(() => {
    if (!runtime?.listening) {
      return;
    }
    const timer = window.setInterval(() => {
      void refreshRuntime(true);
    }, 2500);
    return () => window.clearInterval(timer);
  }, [runtime?.listening]);

  useEffect(() => {
    const calibrationSnapshot = buildCalibrationSnapshot(marks);
    if (lastCalibrationSnapshotRef.current === null) {
      lastCalibrationSnapshotRef.current = calibrationSnapshot;
      return;
    }
    if (lastCalibrationSnapshotRef.current !== calibrationSnapshot) {
      lastCalibrationSnapshotRef.current = calibrationSnapshot;
      setOcrReport(null);
    }
  }, [marks]);

  useEffect(() => {
    const screenshotPath = capture?.pngPath ?? null;
    if (!screenshotPath) {
      resetAllNameOcrPreview();
      return;
    }

    marks.nameRegions.forEach((region, index) => {
      if (!isPixelRect(region)) {
        resetNameOcrPreviewSlot(index);
        return;
      }
      const nextSignature = `${screenshotPath}:${rectSignature(region)}`;
      if (nameOcrSlotKeysRef.current[index] === nextSignature) {
        return;
      }
      nameOcrSlotKeysRef.current[index] = nextSignature;
      const requestId = Date.now() + index;
      nameOcrRequestIdsRef.current[index] = requestId;
      setLiveNameOcr((current) => updateNameOcrSlot(current, index, buildPendingNameOcrPreviewSlot(index)));
      void runNameRegionPrecheck(index, region, screenshotPath, nextSignature, requestId);
    });
  }, [capture?.pngPath, marks.nameRegions]);

  const completedMarks = useMemo(() => {
    return [
      ...marks.nameRegions,
      ...marks.bottomAnchors,
      marks.bottomButtonRegion,
    ].filter(Boolean).length;
  }, [marks]);

  const readyToCheck = completedMarks === regionDefinitions.length && capture !== null;
  const readyToSave = readyToCheck && ocrReport !== null;

  async function boot() {
    const loadedCalibration = await loadCalibrationSilently();
    const runtimeSnapshot = await refreshRuntime(true);
    await loadMonitors();

    if (!loadedCalibration) {
      setAutomationStatus({
        phase: "idle",
        message: "尚未完成校准，完成校准后才会自动开始监听。",
      });
      return;
    }

    if (runtimeSnapshot?.listening) {
      setAutomationStatus({
        phase: "active",
        message: "已检测到后台监听正在运行。",
      });
      return;
    }

    await enqueueControlTask(async () => {
      await ensureListenerRunning({
        trigger: "startup",
        successMessage: "检测到已校准配置，助手已自动开始监听。",
        failureMessage: "检测到已校准配置，但自动监听启动失败",
      });
    });
  }

  async function runManagedCommand<T>(
    key: string,
    command: string,
    args?: Record<string, unknown>,
    exposeError = true,
  ): Promise<{ data: T | null; errorMessage: string | null }> {
    setBusy(key);
    if (exposeError) {
      setError(null);
    }
    try {
      return { data: await invoke<T>(command, args), errorMessage: null };
    } catch (caught) {
      const errorMessage = String(caught);
      if (exposeError) {
        setError(errorMessage);
      }
      return { data: null, errorMessage };
    } finally {
      setBusy(null);
    }
  }

  async function runCommand<T>(
    key: string,
    command: string,
    args?: Record<string, unknown>,
    silent = false,
  ): Promise<T | null> {
    const { data } = await runManagedCommand<T>(key, command, args, !silent);
    return data;
  }

  function updateRuntimeSnapshot(snapshot: RuntimeLoopSnapshot) {
    runtimeRef.current = snapshot;
    setRuntime(snapshot);
  }

  function enqueueControlTask(task: () => Promise<void>) {
    const nextTask = controlQueueRef.current.then(task, task);
    controlQueueRef.current = nextTask.catch(() => undefined);
    return nextTask;
  }

  function formatCommandError(errorMessage: string | null, fallback: string) {
    if (!errorMessage) {
      return fallback;
    }
    return errorMessage;
  }

  async function loadCalibrationSilently() {
    try {
      const loaded = await invoke<CalibrationProfileResult>("load_calibration_profile");
      setProfile(loaded);
      setCalibrated(true);
      setMode("status");
      return true;
    } catch {
      setProfile(null);
      setCalibrated(false);
      setMode("calibration");
      return false;
    }
  }

  async function refreshRuntime(silent = false) {
    const { data } = await runManagedCommand<RuntimeLoopSnapshot>(
      "runtime-status",
      "get_runtime_orchestrator_status",
      undefined,
      !silent,
    );
    if (data) {
      updateRuntimeSnapshot(data);
    }
    return data;
  }

  async function loadMonitors() {
    const data = await runCommand<MonitorDiagnostic[]>("monitors", "list_capture_monitors", undefined, true);
    if (data) {
      setMonitors(data);
      const primary = data.find((monitor) => monitor.primary) ?? data[0];
      if (primary) {
        setSelectedMonitorId(String(primary.id));
      }
    }
  }

  function beginCalibration(message = "请按向导重新完成校准。") {
    setMode("calibration");
    setCapture(null);
    setScreenshot(null);
    setMarks(emptyMarkState());
    setActiveRegion("name-0");
    setCalibrationStepIndex(0);
    setOcrReport(null);
    resetAllNameOcrPreview();
    setError(null);
    setToast({ tone: "info", message });
    void loadMonitors();
  }

  async function ensureListenerRunning({
    trigger,
    successMessage,
    failureMessage,
  }: {
    trigger: "startup" | "calibration-save";
    successMessage: string;
    failureMessage: string;
  }) {
    if (runtimeRef.current?.listening) {
      setAutomationStatus({
        phase: "active",
        message: "后台监听已经在运行，无需重复启动。",
      });
      return true;
    }

    setAutomationStatus({
      phase: "starting",
      message: trigger === "startup" ? "检测到已校准配置，正在自动启动监听。" : "校准已保存，正在恢复监听。",
    });

    const request: RuntimeTriggerRequest = {
      panelSnapshot: { panelState: "collapsed", choices: [], selectedSlot: null },
    };
    const { data, errorMessage } = await runManagedCommand<RuntimeLoopSnapshot>(
      "runtime-start",
      "start_runtime_listener",
      { request },
      false,
    );
    if (data) {
      updateRuntimeSnapshot(data);
      setAutomationStatus({
        phase: "active",
        message: successMessage,
      });
      await getCurrentWindow().hide();
      return true;
    }
    const latest = await refreshRuntime(true);
    if (latest?.listening) {
      setAutomationStatus({
        phase: "active",
        message: successMessage,
      });
      await getCurrentWindow().hide();
      return true;
    }

    setAutomationStatus({
      phase: "error",
      message: "自动监听未启动，请检查客户端是否可用后再试。",
    });
    setToast({
      tone: "error",
      message: `${failureMessage}：${formatCommandError(errorMessage, "请检查客户端连接状态。")}`,
    });
    return false;
  }

  async function stopListenerForCalibration() {
    if (!runtimeRef.current?.listening) {
      setAutomationStatus({
        phase: "paused",
        message: "当前未在监听，已直接进入校准流程。",
      });
      return true;
    }

    setAutomationStatus({
      phase: "stopping",
      message: "正在暂停监听，准备进入校准。",
    });

    const { data, errorMessage } = await runManagedCommand<RuntimeLoopSnapshot>(
      "runtime-stop",
      "stop_runtime_listener",
      undefined,
      false,
    );
    if (data) {
      updateRuntimeSnapshot(data);
      setAutomationStatus({
        phase: "paused",
        message: "监听已暂停，等待重新校准完成。",
      });
      return true;
    }
    const latest = await refreshRuntime(true);
    if (latest?.listening === false) {
      setAutomationStatus({
        phase: "paused",
        message: "监听已暂停，等待重新校准完成。",
      });
      return true;
    }

    setAutomationStatus({
      phase: "error",
      message: "暂停监听失败，暂未进入校准流程。",
    });
    setToast({
      tone: "error",
      message: `暂停监听失败，未进入校准：${formatCommandError(errorMessage, "请稍后重试。")}`,
    });
    return false;
  }

  async function requestRecalibration(message: string) {
    await enqueueControlTask(async () => {
      const stopped = await stopListenerForCalibration();
      if (!stopped) {
        return;
      }
      beginCalibration(message);
    });
  }

  async function captureAfterDelay() {
    const preferredMonitorId = selectedMonitorId === "" ? null : Number.parseInt(selectedMonitorId, 10);
    const appWindow = getCurrentWindow();
    setCountdown(5);
    setToast({
      tone: "info",
      message: "倒计时已经开始，请立刻切回游戏海克斯三选一画面并保持不动，助手会在 5 秒后自动截图。",
    });
    try {
      await appWindow.hide();
    } catch {
    }
    for (let left = 4; left >= 0; left -= 1) {
      await delay(1000);
      setCountdown(left === 0 ? null : left);
    }

    const data = await runCommand<CaptureSampleReport>("capture", "capture_monitor_sample", {
      preferredMonitorId,
    });
    if (!data) {
      try {
        await appWindow.show();
        await appWindow.setFocus();
      } catch {
      }
      return;
    }
    await loadCaptureIntoCalibration(data, "截图完成，可以关闭或离开自定义游戏。");
    try {
      await appWindow.show();
      await appWindow.setFocus();
    } catch {
    }
  }

  async function loadCaptureIntoCalibration(data: CaptureSampleReport, successMessage: string) {
    setCapture(data);
    setOcrReport(null);
    resetAllNameOcrPreview();
    setMarks(emptyMarkState());
    setActiveRegion("name-0");
    setCalibrationStepIndex(calibrationSteps.length - 1);
    const imageData = await runCommand<ScreenshotDataUrl>("screenshot-data", "read_png_file_as_data_url", {
      path: data.pngPath,
    });
    if (imageData) {
      setScreenshot(imageData);
      setToast({ tone: "success", message: successMessage });
    }
  }

  async function runOcrCheck() {
    const input = buildCalibrationInput();
    if (!input || !capture) {
      setError("请先完成全部标记。");
      return;
    }
    const data = await runCommand<CalibratedNameOcrReport>("ocr-check", "run_pixel_calibrated_name_ocr", {
      input,
      screenshotPath: capture.pngPath,
    });
    if (data) {
      setOcrReport(data);
      setToast({ tone: "success", message: "已生成三槽识别校验结果，请确认区域和定位。" });
    }
  }

  async function runNameRegionPrecheck(
    index: number,
    region: PixelRect,
    screenshotPath: string,
    signature: string,
    requestId: number,
  ) {
    try {
      const report = await invoke<NameRegionOcrPrecheckReport>("run_name_region_ocr_precheck", {
        input: {
          screenshotPath,
          nameRegion: region,
        },
      });
      if (nameOcrSlotKeysRef.current[index] !== signature || nameOcrRequestIdsRef.current[index] !== requestId) {
        return;
      }
      setLiveNameOcr((current) =>
        updateNameOcrSlot(current, index, buildResolvedNameOcrPreviewSlot(index, report)),
      );
    } catch (caught) {
      if (nameOcrSlotKeysRef.current[index] !== signature || nameOcrRequestIdsRef.current[index] !== requestId) {
        return;
      }
      setLiveNameOcr((current) =>
        updateNameOcrSlot(current, index, buildFailedNameOcrPreviewSlot(index, String(caught))),
      );
    }
  }

  async function saveCalibration() {
    const input = buildCalibrationInput();
    if (!input) {
      setError("请先完成全部标记。");
      return;
    }
    const data = await runCommand<CalibrationProfileResult>("calibration-save", "save_pixel_calibration_profile", {
      input,
    });
    if (data) {
      setProfile(data);
      setCalibrated(true);
      setMode("status");
      setToast({
        tone: "success",
        message: "校准已保存，正在恢复自动监听。",
      });
      void enqueueControlTask(async () => {
        await ensureListenerRunning({
          trigger: "calibration-save",
          successMessage: "校准已保存，助手已恢复自动监听。",
          failureMessage: "校准已保存，但自动恢复监听失败",
        });
      });
    }
  }

  function clearCalibrationMarks() {
    setMarks(() => emptyMarkState());
    setActiveRegion("name-0");
    setOcrReport(null);
    resetAllNameOcrPreview();
  }

  function resetNameOcrPreviewSlot(index: number) {
    nameOcrSlotKeysRef.current[index] = null;
    nameOcrRequestIdsRef.current[index] = 0;
    setLiveNameOcr((current) => updateNameOcrSlot(current, index, buildIdleNameOcrPreviewSlot(index)));
  }

  function resetAllNameOcrPreview() {
    nameOcrSlotKeysRef.current = [null, null, null];
    nameOcrRequestIdsRef.current = [0, 0, 0];
    setLiveNameOcr(buildEmptyNameOcrPreview());
  }

  function buildCalibrationInput(): CalibrationInput | null {
    if (!capture) {
      return null;
    }
    if (marks.nameRegions.some((region) => !region)) {
      return null;
    }
    if (marks.bottomAnchors.some((point) => !point)) {
      return null;
    }
    if (!marks.bottomButtonRegion) {
      return null;
    }
    return {
      screenshotSize: {
        width: capture.image.width,
        height: capture.image.height,
      },
      nameRegions: marks.nameRegions as [PixelRect, PixelRect, PixelRect],
      bottomAnchors: marks.bottomAnchors as [PixelPoint, PixelPoint, PixelPoint],
      bottomButtonRegion: marks.bottomButtonRegion,
    };
  }

  function renderContent() {
    if (calibrated === null) {
      return <section className="panel loading-panel">正在读取助手状态...</section>;
    }
    if (mode === "calibration" || calibrated === false) {
      return (
        <CalibrationWizard
          monitors={monitors}
          selectedMonitorId={selectedMonitorId}
          onMonitorChange={setSelectedMonitorId}
          capture={capture}
          screenshot={screenshot}
          marks={marks}
          activeRegion={activeRegion}
          setActiveRegion={setActiveRegion}
          calibrationStepIndex={calibrationStepIndex}
          onStepChange={setCalibrationStepIndex}
          setMarks={setMarks}
          completedMarks={completedMarks}
          drag={drag}
          setDrag={setDrag}
          countdown={countdown}
          busy={busy}
          readyToCheck={readyToCheck}
          readyToSave={readyToSave}
          ocrReport={ocrReport}
          nameOcrPreview={mergeNameOcrPreview(liveNameOcr, ocrReport)}
          onResetCalibrationMarks={clearCalibrationMarks}
          onRefreshMonitors={loadMonitors}
          onCapture={captureAfterDelay}
          onRunOcrCheck={runOcrCheck}
          onSave={saveCalibration}
        />
      );
    }
    return (
      <StatusPanel
        profile={profile}
        runtime={runtime}
        automationStatus={automationStatus}
        busy={busy}
        onRefresh={() => void refreshRuntime()}
        onRecalibrate={() => void requestRecalibration("监听已暂停，请重新完成校准。")}
      />
    );
  }

  return (
    <main className="app-shell">
      <div className="app-shell-glow app-shell-glow-left" />
      <div className="app-shell-glow app-shell-glow-right" />
      <header className="topbar">
        <div className="topbar-copy">
          <p className="eyebrow">Northlight Panel</p>
          <h1>{mode === "calibration" || calibrated === false ? "海克斯校准与接管" : "对局助手面板"}</h1>
          <p className="topbar-subtitle">
            {mode === "calibration" || calibrated === false
              ? "把游戏画面、识别区域和底部锚点校准到当前设备，后续识别和 Overlay 才会稳定。"
              : "在局内海克斯阶段自动识别候选，保持低干扰常驻，并把可用信息压到你需要的位置。"}
          </p>
        </div>
        <div className="topbar-badge">
          <span>目标模式</span>
          <strong>海克斯乱斗 / KIWI</strong>
        </div>
      </header>

      {error ? <section className="message error">{error}</section> : null}
      {toast ? <section className={`message ${toast.tone}`}>{toast.message}</section> : null}

      {renderContent()}
    </main>
  );
}

function StatusPanel({
  profile,
  runtime,
  automationStatus,
  busy,
  onRefresh,
  onRecalibrate,
}: {
  profile: CalibrationProfileResult | null;
  runtime: RuntimeLoopSnapshot | null;
  automationStatus: AutomationStatus;
  busy: string | null;
  onRefresh: () => void;
  onRecalibrate: () => void;
}) {
  const runtimeStatus = runtime ? runtimeStatusText[runtime.state.status] ?? runtime.state.status : "待启动";
  const accent = runtime ? runtimeStatusAccent[runtime.state.status] ?? "neutral" : "neutral";
  const pendingTierText =
    runtime && runtime.state.pendingTiers.length > 0 ? runtime.state.pendingTiers.join(" / ") : "无待处理档位";
  const faultText = runtime?.lastErrorCode ?? runtime?.state.pauseReason ?? "链路正常";

  return (
    <section className="status-stack">
      <article className={`panel status-hero status-hero-${accent}`}>
        <div className="status-hero-main">
          <p className="section-kicker">实时态势</p>
          <div className="status-hero-title-row">
            <h2>{runtimeStatus}</h2>
            <span className={`signal-pill signal-pill-${accent}`}>{runtime?.listening ? "监听中" : "待命"}</span>
          </div>
          <p className="status-hero-summary">{automationStatus.message}</p>
          <div className="button-row">
            <button type="button" onClick={onRefresh} disabled={busy !== null}>
              刷新状态
            </button>
            <button type="button" onClick={onRecalibrate} disabled={busy !== null}>
              重新校准
            </button>
          </div>
        </div>
        <div className="status-hero-side">
          <div className="hero-metric">
            <span>待处理海克斯</span>
            <strong>{pendingTierText}</strong>
          </div>
          <div className="hero-metric">
            <span>最近链路状态</span>
            <strong>{faultText}</strong>
          </div>
        </div>
      </article>

      <section className="status-grid">
        <article className="status-card">
          <span>运行状态</span>
          <strong>{runtime?.listening ? "后台监听已接管" : "当前未接管"}</strong>
          <p>用于判断 Tauri 后台监听和运行时编排器是否仍在活动。</p>
        </article>
        <article className="status-card">
          <span>校准状态</span>
          <strong>{profile ? "当前设备已完成校准" : "还没有可用校准"}</strong>
          <p>校准决定截图裁剪区域、底部锚点和 Overlay 定位能否稳定命中。</p>
        </article>
        <article className="status-card">
          <span>当前阶段</span>
          <strong>{runtimeStatus}</strong>
          <p>这里反映的是编排器状态，而不是单次命令结果。</p>
        </article>
        <article className="status-card">
          <span>待处理档位</span>
          <strong>{pendingTierText}</strong>
          <p>有待处理档位时，前端和 Overlay 才会进入更积极的识别节奏。</p>
        </article>
        <article className="status-card">
          <span>异常与暂停</span>
          <strong>{faultText}</strong>
          <p>优先看这里判断是 live client、模式不匹配还是识别链路被暂停。</p>
        </article>
        <article className="status-card status-card-wide">
          <span>自动编排说明</span>
          <strong>{automationStatus.phase}</strong>
          <p>{automationStatus.message}</p>
        </article>
      </section>
    </section>
  );
}

function CalibrationWizard({
  monitors,
  selectedMonitorId,
  onMonitorChange,
  capture,
  screenshot,
  marks,
  activeRegion,
  setActiveRegion,
  calibrationStepIndex,
  onStepChange,
  setMarks,
  completedMarks,
  drag,
  setDrag,
  countdown,
  busy,
  readyToCheck,
  readyToSave,
  ocrReport,
  nameOcrPreview,
  onResetCalibrationMarks,
  onRefreshMonitors,
  onCapture,
  onRunOcrCheck,
  onSave,
}: {
  monitors: MonitorDiagnostic[];
  selectedMonitorId: string;
  onMonitorChange: (value: string) => void;
  capture: CaptureSampleReport | null;
  screenshot: ScreenshotDataUrl | null;
  marks: MarkState;
  activeRegion: RegionKey;
  setActiveRegion: (key: RegionKey) => void;
  calibrationStepIndex: number;
  onStepChange: (index: number) => void;
  setMarks: (setter: (current: MarkState) => MarkState) => void;
  completedMarks: number;
  drag: DragState | null;
  setDrag: (drag: DragState | null) => void;
  countdown: number | null;
  busy: string | null;
  readyToCheck: boolean;
  readyToSave: boolean;
  ocrReport: CalibratedNameOcrReport | null;
  nameOcrPreview: NameOcrPreviewSlot[];
  onResetCalibrationMarks: () => void;
  onRefreshMonitors: () => void;
  onCapture: () => void;
  onRunOcrCheck: () => void;
  onSave: () => void;
}) {
  const activeStep = calibrationSteps[calibrationStepIndex] ?? calibrationSteps[0];
  const isWorkbenchVisible = capture !== null && screenshot !== null;

  if (!isWorkbenchVisible) {
    return (
      <section className="wizard-intro">
        <article className="panel step-panel">
          <div className="step-panel-header">
            <p className="section-kicker">校准步骤</p>
            <span className="step-chip">
              {calibrationStepIndex + 1}/{calibrationSteps.length}
            </span>
          </div>
          <div className="step-progress-bar" aria-hidden="true">
            <span style={{ width: `${((calibrationStepIndex + 1) / calibrationSteps.length) * 100}%` }} />
          </div>
          <div className="step-panel-copy">
            <p className="step-title">{activeStep.title}</p>
            <h2>{activeStep.action}</h2>
            <p className="guide-lead">{activeStep.detail}</p>
          </div>
          {activeStep.id === "capture" ? (
            <div className="capture-controls capture-controls-standalone">
              <label>
                目标显示器
                <select
                  value={selectedMonitorId}
                  onChange={(event) => onMonitorChange(event.target.value)}
                  disabled={busy !== null || monitors.length === 0}
                >
                  <option value="">主显示器</option>
                  {monitors.map((monitor) => (
                    <option key={monitor.id} value={monitor.id}>
                      {monitor.primary ? "主屏 · " : ""}
                      {monitor.friendlyName || monitor.name || `显示器 ${monitor.id}`} · {monitor.width}x
                      {monitor.height}
                    </option>
                  ))}
                </select>
              </label>
              <div className="button-row">
                <button type="button" onClick={onRefreshMonitors} disabled={busy !== null}>
                  刷新显示器
                </button>
                <button type="button" onClick={onCapture} disabled={busy !== null || countdown !== null}>
                  {countdown ? `${countdown} 秒后截图` : "5 秒后截图"}
                </button>
              </div>
              <p className="muted">点击倒计时截图后会立即隐藏助手窗口，请直接切回游戏等待自动截图。</p>
            </div>
          ) : null}
          <div className="button-row step-actions">
            <button
              type="button"
              onClick={() => onStepChange(Math.max(0, calibrationStepIndex - 1))}
              disabled={busy !== null || calibrationStepIndex === 0}
            >
              上一步
            </button>
            {activeStep.id !== "capture" ? (
              <button
                type="button"
                onClick={() => onStepChange(Math.min(calibrationSteps.length - 1, calibrationStepIndex + 1))}
                disabled={busy !== null}
              >
                已完成，下一步
              </button>
            ) : null}
          </div>
        </article>
      </section>
    );
  }

  return (
    <section className="workspace-stack">
      <article className="panel marking-panel">
        <div className="marking-header">
          <div>
            <p className="section-kicker">截图工作台</p>
            <h2>标出名称区域、底部锚点和按钮区域</h2>
            <p>当前：{regionDefinitions.find((item) => item.key === activeRegion)?.label} · {completedMarks}/7</p>
          </div>
          <div className="button-row">
            <button type="button" onClick={onCapture} disabled={busy !== null || countdown !== null}>
              {countdown ? `${countdown} 秒后截图` : "重新截图"}
            </button>
          </div>
        </div>
        <div className="marking-layout">
          <ScreenshotPreview
            screenshot={screenshot}
            capture={capture}
            marks={marks}
            activeRegion={activeRegion}
            drag={drag}
            setDrag={setDrag}
            setMarks={setMarks}
            setActiveRegion={setActiveRegion}
            onClearMarks={onResetCalibrationMarks}
          />
          <CalibrationSidebar
            nameOcrPreview={nameOcrPreview}
            ocrReport={ocrReport}
            busy={busy}
            readyToCheck={readyToCheck}
            readyToSave={readyToSave}
            onRunOcrCheck={onRunOcrCheck}
            onSave={onSave}
          />
        </div>
      </article>
    </section>
  );
}

function ScreenshotPreview({
  screenshot,
  capture,
  marks,
  activeRegion,
  drag,
  setDrag,
  setMarks,
  setActiveRegion,
  onClearMarks,
}: {
  screenshot: ScreenshotDataUrl | null;
  capture: CaptureSampleReport | null;
  marks: MarkState;
  activeRegion: RegionKey;
  drag: DragState | null;
  setDrag: (drag: DragState | null) => void;
  setMarks: (setter: (current: MarkState) => MarkState) => void;
  setActiveRegion: (key: RegionKey) => void;
  onClearMarks: () => void;
}) {
  const ref = useRef<HTMLDivElement | null>(null);
  const width = capture?.image.width ?? 16;
  const height = capture?.image.height ?? 9;
  const activeDefinition = regionDefinitions.find((definition) => definition.key === activeRegion)!;
  const previewScale = 0.5;

  function pointerToPixel(event: React.PointerEvent<HTMLDivElement>) {
    const bounds = ref.current?.getBoundingClientRect();
    if (!bounds) {
      return { x: 0, y: 0 };
    }
    const x = clamp(Math.round(((event.clientX - bounds.left) / bounds.width) * width), 0, width);
    const y = clamp(Math.round(((event.clientY - bounds.top) / bounds.height) * height), 0, height);
    return { x, y };
  }

  function onPointerDown(event: React.PointerEvent<HTMLDivElement>) {
    if (!capture) {
      return;
    }
    const point = pointerToPixel(event);
    if (activeDefinition.kind === "point") {
      setMarks((current) => setRegionValue(current, activeRegion, point));
      return;
    }
    setDrag({
      key: activeRegion,
      startX: point.x,
      startY: point.y,
      currentX: point.x,
      currentY: point.y,
    });
    event.currentTarget.setPointerCapture(event.pointerId);
  }

  function onPointerMove(event: React.PointerEvent<HTMLDivElement>) {
    if (!drag) {
      return;
    }
    const point = pointerToPixel(event);
    setDrag({ ...drag, currentX: point.x, currentY: point.y });
  }

  function onPointerUp() {
    if (!drag) {
      return;
    }
    const rect = rectFromDrag(drag);
    setDrag(null);
    if (rect.width < 4 || rect.height < 4) {
      return;
    }
    setMarks((current) => setRegionValue(current, drag.key, rect));
  }

  const visibleRects = collectRectOverlays(marks, drag);
  const visiblePoints = collectPointOverlays(marks);

  return (
    <div className="preview-column">
      <div className="preview-toolbar-shell">
        <div className="preview-toolbar-head">
          <p className="muted">先选目标，再在截图里框选或落点。</p>
          <button type="button" onClick={onClearMarks}>
            清空标记
          </button>
        </div>
        <div className="region-strip">
          {regionDefinitions.map((definition) => {
            const value = getRegionValue(marks, definition.key);
            return (
              <button
                key={definition.key}
                type="button"
                className={`region-item ${definition.key === activeRegion ? "selected" : ""}`}
                onClick={() => setActiveRegion(definition.key)}
              >
                <span>
                  <strong>{definition.label}</strong>
                  <small>{definition.kind === "point" ? "点击落点" : "拖拽框选"}</small>
                </span>
                <em>{value ? formatRegionValue(value) : "未标记"}</em>
              </button>
            );
          })}
        </div>
      </div>
      <div className="preview-meta">
        <span>{capture ? `截图尺寸：${capture.image.width} x ${capture.image.height}` : "等待截图"}</span>
        <span>{capture ? `截图时间：${capture.capturedAt}` : "完成截图后在这里标记"}</span>
        <span>预览缩放：50%</span>
      </div>
      <div className="preview-viewport">
        <div
          ref={ref}
          className="screenshot-preview"
          style={{
            width: `${Math.max(1, Math.round(width * previewScale))}px`,
            height: `${Math.max(1, Math.round(height * previewScale))}px`,
          }}
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
          onPointerCancel={() => setDrag(null)}
        >
          {screenshot ? (
            <img src={screenshot.dataUrl} alt="校准截图" draggable={false} />
          ) : (
            <div className="preview-placeholder">
              <strong>等待游戏截图</strong>
              <span>倒计时完成后，这里会成为你的校准工作台。</span>
            </div>
          )}
          {visibleRects.map((item) => (
            <div
              key={item.key}
              className={`region-box ${item.key === activeRegion ? "active" : ""}`}
              style={rectToPercentStyle(item.rect, width, height)}
            >
              <span>{item.label}</span>
            </div>
          ))}
          {visiblePoints.map((item) => (
            <div
              key={item.key}
              className={`point-marker ${item.key === activeRegion ? "active" : ""}`}
              style={pointToPercentStyle(item.point, width, height)}
            >
              <span>{item.label}</span>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

function CalibrationSidebar({
  nameOcrPreview,
  ocrReport,
  busy,
  readyToCheck,
  readyToSave,
  onRunOcrCheck,
  onSave,
}: {
  nameOcrPreview: NameOcrPreviewSlot[];
  ocrReport: CalibratedNameOcrReport | null;
  busy: string | null;
  readyToCheck: boolean;
  readyToSave: boolean;
  onRunOcrCheck: () => void;
  onSave: () => void;
}) {
  return (
    <aside className="calibration-side">
      <section className="side-block">
        <div className="side-block-head">
          <div>
            <p className="section-kicker">实时 OCR</p>
            <h2>右侧随时看预检</h2>
          </div>
        </div>
        <div className="ocr-preview-list">
          {nameOcrPreview.map((slot) => (
            <article
              key={slot.slot}
              className={`ocr-preview-card ${slot.lowConfidence ? "warn" : ""} ${slot.status === "pending" ? "pending" : ""} ${slot.status === "error" ? "error" : ""}`}
            >
              <div className="ocr-preview-head">
                <strong>{slotText[slot.slot]}</strong>
                <span>{slot.source === "ocr-check" ? "校验" : "实时"}</span>
              </div>
              <p>{slot.status === "pending" ? "正在预检..." : slot.text ?? "等待名称结果"}</p>
              <small>{slot.status === "pending" ? "框选后自动触发" : formatNameOcrMetrics(slot)}</small>
            </article>
          ))}
        </div>
      </section>

      <section className="side-block">
        <div className="side-block-head side-block-head-actions">
          <div>
            <p className="section-kicker">识别校验</p>
            <h2>确认后保存</h2>
          </div>
          <button type="button" onClick={onRunOcrCheck} disabled={busy !== null || !readyToCheck}>
            生成校验结果
          </button>
        </div>
        {ocrReport ? (
          <div className="ocr-grid ocr-grid-compact">
            {ocrReport.slots.map((slot) => (
              <div key={slot.slot} className={slot.finalName ? "ocr-card pass" : "ocr-card warn"}>
                <strong>{slotText[slot.slot]}</strong>
                <span>{slot.finalName ?? "未确认"}</span>
                <small>
                  原文 {slot.rawText || "-"} · 置信度 {slot.confidence.toFixed(2)} · 匹配 {slot.matchScore.toFixed(2)}
                  {slot.failureReason ? ` · ${slot.failureReason}` : ""}
                </small>
              </div>
            ))}
          </div>
        ) : (
          <p className="muted">7 个标记完成后生成校验。</p>
        )}
        <div className="button-row final-row">
          <button type="button" onClick={onSave} disabled={busy !== null || !readyToSave}>
            确认并保存配置
          </button>
        </div>
      </section>
    </aside>
  );
}

function mergeNameOcrPreview(
  liveNameOcr: NameOcrPreviewSlot[],
  ocrReport: CalibratedNameOcrReport | null,
): NameOcrPreviewSlot[] {
  const merged = liveNameOcr.map((slot) => ({ ...slot }));
  if (!ocrReport) {
    return merged;
  }
  ocrReport.slots.forEach((slot, index) => {
    const current = merged[index];
    if (current && current.status === "ready") {
      return;
    }
    merged[index] = {
      slot: slot.slot,
      text: slot.finalName ?? (slot.rawText || null),
      rawText: slot.rawText || null,
      confidence: slot.confidence,
      matchScore: slot.matchScore,
      lowConfidence: slot.confidence < 0.75 || Boolean(slot.failureReason),
      hint:
        slot.failureReason ??
        (slot.confidence < 0.75
          ? "置信度偏低，请调整到名称文字本体并避开边框。"
          : "名称区域可用于后续实时展示。"),
      source: "ocr-check",
      status: "ready",
    };
  });
  return merged;
}

function buildCalibrationSnapshot(marks: MarkState): string {
  return JSON.stringify(marks);
}

function isPixelRect(region: PixelRect | null): region is PixelRect {
  return region !== null;
}

function rectSignature(rect: PixelRect): string {
  return `${rect.x}:${rect.y}:${rect.width}:${rect.height}`;
}

function buildEmptyNameOcrPreview(): NameOcrPreviewSlot[] {
  return [0, 1, 2].map((index) => buildIdleNameOcrPreviewSlot(index)) as NameOcrPreviewSlot[];
}

function buildIdleNameOcrPreviewSlot(index: number): NameOcrPreviewSlot {
  return {
    slot: nameSlotAt(index),
    text: null,
    rawText: null,
    confidence: null,
    matchScore: null,
    lowConfidence: false,
    hint: "名称框完成后会自动跑单区域 OCR 预检。",
    source: "placeholder",
    status: "idle",
  };
}

function buildPendingNameOcrPreviewSlot(index: number): NameOcrPreviewSlot {
  return {
    ...buildIdleNameOcrPreviewSlot(index),
    source: "live",
    status: "pending",
    hint: "正在根据当前名称框做 OCR 预检。",
  };
}

function buildResolvedNameOcrPreviewSlot(
  index: number,
  report: NameRegionOcrPrecheckReport,
): NameOcrPreviewSlot {
  const isLowConfidence =
    report.belowConfidenceThreshold || report.belowMatchThreshold || report.requiresUserAdjustment;
  return {
    slot: nameSlotAt(index),
    text: report.finalName ?? (report.rawText || "未命中名称"),
    rawText: report.rawText || null,
    confidence: report.confidence,
    matchScore: report.matchScore,
    lowConfidence: isLowConfidence,
    hint: buildNameOcrHint(report),
    source: "live",
    status: "ready",
  };
}

function buildFailedNameOcrPreviewSlot(index: number, errorMessage: string): NameOcrPreviewSlot {
  return {
    slot: nameSlotAt(index),
    text: "预检失败",
    rawText: null,
    confidence: null,
    matchScore: null,
    lowConfidence: true,
    hint: `OCR 预检失败：${errorMessage}`,
    source: "live",
    status: "error",
  };
}

function buildNameOcrHint(report: NameRegionOcrPrecheckReport): string {
  if (report.requiresUserAdjustment || report.belowConfidenceThreshold || report.belowMatchThreshold) {
    const reasons: string[] = [];
    if (report.failureReason) {
      reasons.push(report.failureReason);
    }
    if (report.belowConfidenceThreshold) {
      reasons.push("置信度偏低");
    }
    if (report.belowMatchThreshold) {
      reasons.push("匹配度偏低");
    }
    return `${reasons.join("，")}，请缩紧到名称文字并避开卡片边框、粒子和图标。`;
  }
  if (report.finalName) {
    return "名称区域命中稳定，可以继续标记其他区域。";
  }
  return "未命中名称，请缩紧到名称文字并避开卡片装饰。";
}

function updateNameOcrSlot(
  slots: NameOcrPreviewSlot[],
  index: number,
  nextSlot: NameOcrPreviewSlot,
): NameOcrPreviewSlot[] {
  return slots.map((slot, slotIndex) => (slotIndex === index ? nextSlot : slot));
}

function nameSlotAt(index: number): "left" | "center" | "right" {
  return index === 0 ? "left" : index === 1 ? "center" : "right";
}

function formatNameOcrMetrics(slot: NameOcrPreviewSlot): string {
  const parts: string[] = [];
  if (slot.rawText) {
    parts.push(`原文 ${slot.rawText}`);
  }
  if (slot.confidence !== null) {
    parts.push(`置信度 ${(slot.confidence * 100).toFixed(0)}%`);
  }
  if (slot.matchScore !== null) {
    parts.push(`匹配 ${slot.matchScore.toFixed(2)}`);
  }
  return parts.length > 0 ? parts.join(" · ") : "置信度待返回";
}

function OverlayPage() {
  const [payload, setPayload] = useState<OverlayPagePayload>(() => {
    return normalizeOverlayPayload(
      window.__HEX_OVERLAY_BOOTSTRAP__ ?? {
        generatedAt: new Date().toISOString(),
        mode: "fallback",
        cards: fallbackOverlayCards(),
      },
    );
  });

  useEffect(() => {
    document.documentElement.classList.add("overlay-document");
    function handleSlotUpdate(event: Event) {
      const customEvent = event as CustomEvent<OverlaySlotData[]>;
      setPayload((current) => ({
        ...current,
        generatedAt: new Date().toISOString(),
        mode: "slotData",
        cards: normalizeOverlayCards(
          current.cards.map((card) => {
            const next = customEvent.detail.find((slot) => slot.slot === card.slot);
            return next ? mergeOverlayCard(card, next) : card;
          }),
        ),
      }));
    }
    window.addEventListener("hex-overlay-slots", handleSlotUpdate);
    return () => {
      document.documentElement.classList.remove("overlay-document");
      window.removeEventListener("hex-overlay-slots", handleSlotUpdate);
    };
  }, []);

  return (
    <main className="overlay-root" aria-label="海克斯推荐 Overlay">
      {payload.cards.map((card) => {
        const viewModel = buildOverlayCardViewModel(card, payload);
        return (
          <article
            key={card.slot}
            className="overlay-card"
            data-mode={payload.mode}
            data-tone={viewModel.tone}
            style={{
              left: `${card.bounds.x}px`,
              top: `${card.bounds.y}px`,
              width: `${card.bounds.width}px`,
              height: `${card.bounds.height}px`,
            }}
          >
            <header className="overlay-card-header">
              <div className="overlay-card-kicker">
                <span>{viewModel.slotLabel}</span>
                <span>{viewModel.statusText}</span>
              </div>
            </header>
            <div className="overlay-card-content">
              <div className="overlay-card-title-row">
                <h1>{viewModel.title}</h1>
                <strong>{viewModel.rankBadgeText}</strong>
              </div>
              <p className="overlay-card-summary">{viewModel.summary}</p>
              {viewModel.detail ? <p className="overlay-card-detail">{viewModel.detail}</p> : null}
              {viewModel.insight ? <p className="overlay-card-insight">{viewModel.insight}</p> : null}
            </div>
            <div className="overlay-card-meta">
              {viewModel.scoreText ? <span>{viewModel.scoreText}</span> : null}
              {viewModel.augmentText ? <span>{viewModel.augmentText}</span> : null}
              {viewModel.rankText ? <span>{viewModel.rankText}</span> : null}
            </div>
            {viewModel.tips.length > 0 ? (
              <div className="overlay-card-tips" aria-label="补充提示">
                {viewModel.tips.map((tip, index) => (
                  <span key={`${card.slot}-tip-${index}`}>{tip}</span>
                ))}
              </div>
            ) : null}
            <footer className="overlay-card-footer">
              <div className="overlay-card-source">
                <span>{viewModel.sourceText}</span>
                {viewModel.sourceDetail ? <small>{viewModel.sourceDetail}</small> : null}
              </div>
              <span>{viewModel.updateText}</span>
            </footer>
          </article>
        );
      })}
    </main>
  );
}

function setRegionValue(current: MarkState, key: RegionKey, value: PixelRect | PixelPoint): MarkState {
  if (key.startsWith("name-")) {
    const index = Number.parseInt(key.slice(-1), 10);
    const nameRegions = [...current.nameRegions] as [PixelRect | null, PixelRect | null, PixelRect | null];
    nameRegions[index] = value as PixelRect;
    return { ...current, nameRegions };
  }
  if (key.startsWith("anchor-")) {
    const index = Number.parseInt(key.slice(-1), 10);
    const bottomAnchors = [...current.bottomAnchors] as [PixelPoint | null, PixelPoint | null, PixelPoint | null];
    bottomAnchors[index] = value as PixelPoint;
    return { ...current, bottomAnchors };
  }
  return { ...current, bottomButtonRegion: value as PixelRect };
}

function getRegionValue(marks: MarkState, key: RegionKey): PixelRect | PixelPoint | null {
  if (key.startsWith("name-")) {
    return marks.nameRegions[Number.parseInt(key.slice(-1), 10)];
  }
  if (key.startsWith("anchor-")) {
    return marks.bottomAnchors[Number.parseInt(key.slice(-1), 10)];
  }
  return marks.bottomButtonRegion;
}

function collectRectOverlays(marks: MarkState, drag: DragState | null) {
  const overlays = regionDefinitions.flatMap((definition) => {
    if (definition.kind !== "rect") {
      return [];
    }
    const value = getRegionValue(marks, definition.key);
    if (!value || !("width" in value)) {
      return [];
    }
    return [{ key: definition.key, label: definition.label, rect: value }];
  });
  if (drag) {
    const definition = regionDefinitions.find((item) => item.key === drag.key);
    overlays.push({ key: drag.key, label: definition?.label ?? "当前区域", rect: rectFromDrag(drag) });
  }
  return overlays;
}

function collectPointOverlays(marks: MarkState) {
  return regionDefinitions.flatMap((definition) => {
    if (definition.kind !== "point") {
      return [];
    }
    const value = getRegionValue(marks, definition.key);
    if (!value || "width" in value) {
      return [];
    }
    return [{ key: definition.key, label: definition.label, point: value }];
  });
}

function rectFromDrag(drag: DragState): PixelRect {
  const x = Math.min(drag.startX, drag.currentX);
  const y = Math.min(drag.startY, drag.currentY);
  return {
    x,
    y,
    width: Math.abs(drag.currentX - drag.startX),
    height: Math.abs(drag.currentY - drag.startY),
  };
}

function rectToPercentStyle(rect: PixelRect, width: number, height: number) {
  return {
    left: `${(rect.x / width) * 100}%`,
    top: `${(rect.y / height) * 100}%`,
    width: `${(rect.width / width) * 100}%`,
    height: `${(rect.height / height) * 100}%`,
  };
}

function pointToPercentStyle(point: PixelPoint, width: number, height: number) {
  return {
    left: `${(point.x / width) * 100}%`,
    top: `${(point.y / height) * 100}%`,
  };
}

function formatRegionValue(value: PixelRect | PixelPoint): string {
  if ("width" in value) {
    return `${value.x}, ${value.y}, ${value.width} x ${value.height}`;
  }
  return `${value.x}, ${value.y}`;
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

function normalizeOverlayPayload(payload: OverlayPagePayload): OverlayPagePayload {
  return {
    ...payload,
    cards: normalizeOverlayCards(payload.cards),
  };
}

function normalizeOverlayCards(cards: OverlayCardInfo[]): OverlayCardInfo[] {
  return cards.map((card) => normalizeOverlayCard(card));
}

function normalizeOverlayCard(card: OverlayCardInfo): OverlayCardInfo {
  return {
    ...card,
    title: normalizeOverlayText(card.title, `槽位 ${card.slot}`),
    body: normalizeOverlayText(card.body, "等待推荐摘要"),
    summary: normalizeOptionalOverlayText(card.summary),
    tips: normalizeOverlayTips(card.tips),
    sourceLabel: normalizeOptionalOverlayText(card.sourceLabel),
    sourceDetail: normalizeOptionalOverlayText(card.sourceDetail),
    insight: normalizeOptionalOverlayText(card.insight),
  };
}

function mergeOverlayCard(card: OverlayCardInfo, next: OverlaySlotData): OverlayCardInfo {
  return normalizeOverlayCard({
    ...card,
    title: next.title,
    body: next.body ?? card.body,
    augmentId: next.augmentId ?? card.augmentId,
    rank: next.rank ?? card.rank,
    score: next.score ?? card.score,
    summary: next.summary ?? card.summary,
    tips: next.tips ?? card.tips,
    sourceLabel: next.sourceLabel ?? card.sourceLabel,
    sourceDetail: next.sourceDetail ?? card.sourceDetail,
    insight: next.insight ?? card.insight,
  });
}

function buildOverlayCardViewModel(card: OverlayCardInfo, payload: OverlayPagePayload): OverlayCardViewModel {
  const summary = card.summary ?? card.body;
  const detail = card.summary && card.body !== card.summary ? card.body : null;
  const sourceText = card.sourceLabel ?? formatOverlayModeText(payload.mode, card.source);
  const rankText = normalizeOptionalOverlayText(card.rank);
  const tone = resolveOverlayTone(payload.mode, rankText, card.score);
  return {
    slotLabel: formatOverlaySlotLabel(card.slot),
    title: card.title,
    summary,
    detail,
    insight: card.insight ?? null,
    tips: card.tips ?? [],
    sourceText,
    sourceDetail: card.sourceDetail ?? null,
    statusText: payload.mode === "fallback" ? "等待接线" : "推荐已更新",
    updateText: formatOverlayUpdateText(payload.generatedAt),
    scoreText: card.score ? `均分 ${card.score}` : null,
    augmentText: card.augmentId ? `ID ${card.augmentId}` : null,
    rankText,
    rankBadgeText: rankText ?? (payload.mode === "fallback" ? "待评级" : "待补充"),
    tone,
  };
}

function normalizeOverlayText(value: string | null | undefined, fallback: string): string {
  const text = value?.trim();
  return text && text.length > 0 ? text : fallback;
}

function normalizeOptionalOverlayText(value: string | null | undefined): string | null {
  const text = value?.trim();
  return text && text.length > 0 ? text : null;
}

function normalizeOverlayTips(value: string[] | null | undefined): string[] {
  if (!Array.isArray(value)) {
    return [];
  }
  return value.map((item) => item.trim()).filter((item) => item.length > 0).slice(0, 3);
}

function formatOverlaySlotLabel(slot: number): string {
  if (slot === 1) {
    return "左侧候选";
  }
  if (slot === 2) {
    return "中间候选";
  }
  if (slot === 3) {
    return "右侧候选";
  }
  return `槽位 ${slot}`;
}

function formatOverlayModeText(mode: string, source: string): string {
  if (mode === "fallback") {
    return "前端兜底";
  }
  if (mode === "static") {
    return "静态预览";
  }
  if (mode === "slotData") {
    return "实时推荐";
  }
  return source;
}

function formatOverlayUpdateText(generatedAt: string): string {
  const date = new Date(generatedAt);
  if (Number.isNaN(date.getTime())) {
    return "更新时间未知";
  }
  return `更新 ${date.toLocaleTimeString("zh-CN", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  })}`;
}

function resolveOverlayTone(
  mode: string,
  rankText: string | null,
  scoreText: string | null | undefined,
): "fallback" | "neutral" | "strong" {
  if (mode === "fallback") {
    return "fallback";
  }
  const normalizedRank = rankText?.toUpperCase() ?? "";
  if (normalizedRank.includes("S") || normalizedRank.includes("T0")) {
    return "strong";
  }
  if (normalizedRank.includes("A") || normalizedRank.includes("T1")) {
    return "strong";
  }
  const score = Number.parseFloat(scoreText ?? "");
  if (!Number.isNaN(score) && score >= 4.2) {
    return "strong";
  }
  return "neutral";
}

function fallbackOverlayCards(): OverlayCardInfo[] {
  return [1, 2, 3].map((slot, index) => ({
    slot,
    title: slot === 1 ? "等待左侧推荐" : slot === 2 ? "等待中间推荐" : "等待右侧推荐",
    body: "当前槽位尚未收到后端推荐，接线完成后会在这里补齐正式说明、来源和操作提示。",
    summary: "这里会展示海克斯名、评级结论和一眼能看懂的推荐摘要。",
    insight:
      slot === 2
        ? "正式接线后支持逐槽刷新，未更新的卡片会继续保留上一帧稳定展示。"
        : "如果这张卡暂时没有数据，页面会保留结构和占位，避免 Overlay 在对局内抖动。",
    tips:
      slot === 2
        ? ["保持三张海克斯卡完全展开", "后端未就绪时仍保留卡位", "来源和提示会在接线后自动补齐"]
        : ["支持后端逐槽更新", "兜底状态也会显示更新时间"],
    augmentId: null,
    rank: null,
    score: null,
    sourceLabel: "前端兜底",
    sourceDetail: "等待后端事件推送",
    bounds: {
      x: 80 + index * 280,
      y: 120,
      width: 260,
      height: 210,
    },
    source: "frontend.fallback",
  }));
}

export default App;
