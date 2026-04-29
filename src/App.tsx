import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

declare global {
  interface Window {
    __HEX_OVERLAY_BOOTSTRAP__?: OverlayPagePayload;
  }
}

type DirectoryStatus = {
  key: string;
  path: string;
  exists: boolean;
};

type RuntimeOverview = {
  appDataDir: string;
  settingsPath: string;
  latestLogPath: string;
  settings: {
    language: string;
    capture: {
      pollIntervalMs: number;
      retryDelayMs: number;
      defaultDisplayMode: string;
    };
    ocr: {
      engine: string;
      minConfidence: number;
      minMatchScore: number;
    };
    overlay: {
      enabled: boolean;
      clickThrough: boolean;
      gap: number;
      maxHeight: number;
    };
    apexLol: {
      cacheTtlHours: number;
      requestTimeoutMs: number;
      failedCacheTtlMinutes: number;
    };
  };
  directories: DirectoryStatus[];
};

type HealthStatus = "pass" | "warn" | "fail" | "notChecked";

type HealthCheckItem = {
  key: string;
  name: string;
  status: HealthStatus;
  details: string;
  errorCode?: string | null;
};

type HealthCheckReport = {
  traceId: string;
  generatedAt: string;
  items: HealthCheckItem[];
};

type ApexParseStatus = "ok" | "noData" | "requestFailed" | "parseFailed";

type DiagnosticExportResult = {
  traceId: string;
  zipPath: string;
  includedFiles: number;
};

type MonitorDiagnostic = {
  id: number;
  name: string;
  friendlyName: string;
  x: number;
  y: number;
  width: number;
  height: number;
  rotation: number;
  scaleFactor: number;
  frequency: number;
  primary: boolean;
  builtin: boolean;
};

type CaptureSampleReport = {
  capturedAt: string;
  monitor: MonitorDiagnostic;
  image: {
    width: number;
    height: number;
    captureDurationMs: number;
    saveDurationMs: number;
    meanLuma: number;
    minLuma: number;
    maxLuma: number;
    brightPixelRatio: number;
    blackScreen: boolean;
    frameHash: string;
  };
  pngPath: string;
  jsonPath: string;
  previousFrameHash?: string | null;
  staleFrame: boolean;
};

type ScreenshotSize = {
  width: number;
  height: number;
};

type NormalizedRect = {
  x: number;
  y: number;
  width: number;
  height: number;
};

type NormalizedPoint = {
  x: number;
  y: number;
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

type CalibrationProfileResult = {
  path: string;
  config: {
    version: number;
    screenshotSize: ScreenshotSize;
    nameRegions: [NormalizedRect, NormalizedRect, NormalizedRect];
    bottomAnchors: [NormalizedPoint, NormalizedPoint, NormalizedPoint];
    bottomButtonRegion: NormalizedRect;
    coordinateSpace: string;
  };
  echo: {
    screenshotSize: ScreenshotSize;
    nameRegionPixels: [PixelRect, PixelRect, PixelRect];
    bottomAnchorPixels: [PixelPoint, PixelPoint, PixelPoint];
    bottomButtonRegionPixels: PixelRect;
    nameRegions: [NormalizedRect, NormalizedRect, NormalizedRect];
    bottomAnchors: [NormalizedPoint, NormalizedPoint, NormalizedPoint];
    bottomButtonRegion: NormalizedRect;
  };
};

type OcrResourceStatus = {
  engine: string;
  modelPath: string;
  modelExists: boolean;
  ready: boolean;
  errorCode?: string | null;
  message: string;
};

type OfflineReplayReport = {
  engine: string;
  slotCount: number;
  minConfidence: number;
  minMatchScore: number;
  slots: Array<{
    slot: "left" | "center" | "right";
    rawText: string;
    confidence: number;
    matchScore: number;
    finalName?: string | null;
    augmentId?: string | null;
    elapsedMs: number;
    failureReason?: string | null;
  }>;
};

type ActivePlayerSnapshot = {
  championName: string;
  level: number;
};

type StateMachineResult = {
  state: {
    status: string;
    player?: ActivePlayerSnapshot | null;
    pendingTier?: number | null;
    pendingTiers?: number[];
    completedTiers: number[];
    visibleChoices: Record<string, string>;
    pauseReason?: string | null;
  };
  events: Array<{
    kind: string;
    fromStatus: string;
    toStatus: string;
    tier?: number | null;
    slot?: number | null;
    previousValue?: string | null;
    nextValue?: string | null;
    reason?: string | null;
  }>;
};

type RuntimeLoopSnapshot = {
  listening: boolean;
  state: {
    status: string;
    player?: ActivePlayerSnapshot | null;
    pendingTier?: number | null;
    pendingTiers: number[];
    completedTiers: number[];
    panelState: string;
    visibleChoices: Record<string, string>;
    pauseReason?: string | null;
  };
  panelSnapshot: {
    panelState: "expanded" | "collapsed";
    choices: Array<{ slot: number; augmentId: string }>;
    selectedSlot?: number | null;
  };
  recentEvents: Array<{
    traceId: string;
    occurredAt: string;
    triggerEvent: string;
    championName?: string | null;
    level?: number | null;
    pendingTiers: number[];
    slotChanges: Array<{
      slot: number;
      previousValue?: string | null;
      nextValue?: string | null;
    }>;
    errorCode?: string | null;
    message: string;
  }>;
  lastErrorCode?: string | null;
};

type ApexCacheReport = {
  cachePath: string;
  reportPath?: string | null;
  generatedAt: string;
  totalEntries: number;
  okEntries: number;
  failedEntries: number;
  expiredEntries: number;
  entries: Array<{
    cacheKey: string;
    championName: string;
    augmentName: string;
    rating?: string | null;
    summary: string;
    tip?: string | null;
    source: string;
    sourceUrl: string;
    requestUrl: string;
    fetchedAt: string;
    expiresAt: string;
    status: ApexParseStatus;
    expired: boolean;
    durationMs: number;
    error?: string | null;
  }>;
};

type ApexLookupResult = {
  cacheKey: string;
  championName: string;
  augmentName: string;
  rating?: string | null;
  summary: string;
  tip?: string | null;
  source: string;
  sourceUrl: string;
  fetchedAt: string;
  expiresAt: string;
  cacheHit: boolean;
  status: ApexParseStatus;
  error?: string | null;
  requestLog: {
    requestUrl: string;
    durationMs: number;
    cacheHit: boolean;
    parseStatus: ApexParseStatus;
    failureReason?: string | null;
  };
};

type OverlayOperationReport = {
  label: string;
  created: boolean;
  visible: boolean;
  monitor: {
    name?: string | null;
    scaleFactor: string;
    position: { x: number; y: number };
    size: { width: number; height: number };
    workArea: { x: number; y: number; width: number; height: number };
  };
  bounds: {
    x: number;
    y: number;
    width: number;
    height: number;
  };
  logicalBounds: {
    x: number;
    y: number;
    width: number;
    height: number;
  };
  cards: OverlayCardInfo[];
  clickThrough: {
    requested: boolean;
    platform: string;
    status: string;
    message: string;
    childWindowResults: Array<{
      phase: string;
      delayMs?: number | null;
      appliedCount: number;
      details: string[];
    }>;
  };
  logPath?: string | null;
  messages: string[];
};

type OverlaySlotData = {
  slot: number;
  title: string;
  body?: string | null;
  augmentId?: string | null;
  rank?: string | null;
  score?: string | null;
};

type OverlayCardInfo = OverlaySlotData & {
  body: string;
  bounds: {
    x: number;
    y: number;
    width: number;
    height: number;
  };
  source: string;
};

type OverlayPagePayload = {
  generatedAt: string;
  mode: string;
  cards: OverlayCardInfo[];
};

type OverlaySlotUpdateReport = {
  label: string;
  visible: boolean;
  updatedSlots: OverlaySlotData[];
  logPath?: string | null;
  message: string;
};

const statusText: Record<HealthStatus, string> = {
  pass: "通过",
  warn: "待补齐",
  fail: "失败",
  notChecked: "未检查",
};

const statusClass: Record<HealthStatus, string> = {
  pass: "pass",
  warn: "warn",
  fail: "fail",
  notChecked: "idle",
};

const safeBoundaries = [
  "不注入",
  "不 Hook",
  "不读内存",
  "不自动点击",
  "不自动选择",
  "不模拟键鼠",
  "不默认枚举进程或窗口标题",
  "不伪造 ApexLOL 数据",
];

const replaySlotLabel: Record<"left" | "center" | "right", string> = {
  left: "左侧",
  center: "中间",
  right: "右侧",
};

const assistantStatusText: Record<string, string> = {
  waitingForGame: "等待对局",
  waitingForTier: "等待档位",
  pendingSelection: "待处理",
  paused: "异常暂停",
};

const runtimeTriggerText: Record<string, string> = {
  manual: "手动触发",
  lowFrequencyPoll: "低频监听",
  listenerStarted: "启动监听",
  listenerStopped: "停止监听",
};

type ErrorState = {
  code: string;
  message: string;
};

type RectFields = {
  x: string;
  y: string;
  width: string;
  height: string;
};

type PointFields = {
  x: string;
  y: string;
};

type CalibrationForm = {
  screenshotWidth: string;
  screenshotHeight: string;
  nameRegions: [RectFields, RectFields, RectFields];
  bottomAnchors: [PointFields, PointFields, PointFields];
  bottomButtonRegion: RectFields;
};

const nameRegionLabels = ["左侧名称", "中间名称", "右侧名称"] as const;
const anchorLabels = ["左侧锚点", "中间锚点", "右侧锚点"] as const;

function createCalibrationForm(width = 1920, height = 1080): CalibrationForm {
  return {
    screenshotWidth: String(width),
    screenshotHeight: String(height),
    nameRegions: [
      { x: "420", y: "350", width: "260", height: "64" },
      { x: "830", y: "350", width: "260", height: "64" },
      { x: "1240", y: "350", width: "260", height: "64" },
    ],
    bottomAnchors: [
      { x: "520", y: "900" },
      { x: "960", y: "900" },
      { x: "1400", y: "900" },
    ],
    bottomButtonRegion: { x: "760", y: "880", width: "400", height: "110" },
  };
}

function extractErrorState(caught: unknown): ErrorState {
  const message = String(caught);
  const match = message.match(/\bHEX-[A-Z0-9-]+/);
  return {
    code: match?.[0] ?? "HEX-UI-COMMAND",
    message,
  };
}

function parseUint(value: string, label: string): number {
  const parsed = Number.parseInt(value, 10);
  if (!Number.isFinite(parsed) || parsed < 0) {
    throw new Error(`HEX-CALIBRATION-FORM: ${label} 必须是非负整数`);
  }
  return parsed;
}

function rectFromFields(rect: RectFields, label: string): PixelRect {
  return {
    x: parseUint(rect.x, `${label} X`),
    y: parseUint(rect.y, `${label} Y`),
    width: parseUint(rect.width, `${label} 宽度`),
    height: parseUint(rect.height, `${label} 高度`),
  };
}

function pointFromFields(point: PointFields, label: string): PixelPoint {
  return {
    x: parseUint(point.x, `${label} X`),
    y: parseUint(point.y, `${label} Y`),
  };
}

function calibrationFormFromEcho(result: CalibrationProfileResult): CalibrationForm {
  return {
    screenshotWidth: String(result.echo.screenshotSize.width),
    screenshotHeight: String(result.echo.screenshotSize.height),
    nameRegions: result.echo.nameRegionPixels.map((rect) => ({
      x: String(rect.x),
      y: String(rect.y),
      width: String(rect.width),
      height: String(rect.height),
    })) as [RectFields, RectFields, RectFields],
    bottomAnchors: result.echo.bottomAnchorPixels.map((point) => ({
      x: String(point.x),
      y: String(point.y),
    })) as [PointFields, PointFields, PointFields],
    bottomButtonRegion: {
      x: String(result.echo.bottomButtonRegionPixels.x),
      y: String(result.echo.bottomButtonRegionPixels.y),
      width: String(result.echo.bottomButtonRegionPixels.width),
      height: String(result.echo.bottomButtonRegionPixels.height),
    },
  };
}

function formatRect(rect: NormalizedRect): string {
  return `${rect.x.toFixed(4)},${rect.y.toFixed(4)},${rect.width.toFixed(4)}x${rect.height.toFixed(4)}`;
}

function formatPoint(point: NormalizedPoint): string {
  return `${point.x.toFixed(4)},${point.y.toFixed(4)}`;
}

const apexStatusText: Record<ApexParseStatus, string> = {
  ok: "成功",
  noData: "暂无数据",
  requestFailed: "请求失败",
  parseFailed: "解析失败",
};

const apexStatusClass: Record<ApexParseStatus, string> = {
  ok: "pass",
  noData: "idle",
  requestFailed: "fail",
  parseFailed: "warn",
};

function App() {
  const isOverlayView = new URLSearchParams(window.location.search).get("view") === "overlay";
  return isOverlayView ? <OverlayPage /> : <DiagnosticApp />;
}

function DiagnosticApp() {
  const [overview, setOverview] = useState<RuntimeOverview | null>(null);
  const [health, setHealth] = useState<HealthCheckReport | null>(null);
  const [diagnosticExport, setDiagnosticExport] = useState<DiagnosticExportResult | null>(null);
  const [releaseExport, setReleaseExport] = useState<DiagnosticExportResult | null>(null);
  const [monitors, setMonitors] = useState<MonitorDiagnostic[]>([]);
  const [selectedMonitorId, setSelectedMonitorId] = useState<string>("");
  const [captureReport, setCaptureReport] = useState<CaptureSampleReport | null>(null);
  const [calibrationForm, setCalibrationForm] = useState<CalibrationForm>(() => createCalibrationForm());
  const [calibrationResult, setCalibrationResult] = useState<CalibrationProfileResult | null>(null);
  const [ocrStatus, setOcrStatus] = useState<OcrResourceStatus | null>(null);
  const [ocrReplay, setOcrReplay] = useState<OfflineReplayReport | null>(null);
  const [liveClient, setLiveClient] = useState<ActivePlayerSnapshot | null>(null);
  const [stateResult, setStateResult] = useState<StateMachineResult | null>(null);
  const [runtimeLoop, setRuntimeLoop] = useState<RuntimeLoopSnapshot | null>(null);
  const [apexResult, setApexResult] = useState<ApexLookupResult | null>(null);
  const [apexReport, setApexReport] = useState<ApexCacheReport | null>(null);
  const [overlayReport, setOverlayReport] = useState<OverlayOperationReport | null>(null);
  const [overlayUpdateReport, setOverlayUpdateReport] = useState<OverlaySlotUpdateReport | null>(
    null,
  );
  const [ocrTexts, setOcrTexts] = useState({
    leftText: "棱彩门票",
    centerText: "好事成双",
    rightText: "利滚利",
  });
  const [overlaySlots, setOverlaySlots] = useState([
    {
      title: "棱彩门票",
      body: "当前英雄优先级高，经济线更稳定",
      rank: "S",
      score: "4.55",
    },
    {
      title: "好事成双",
      body: "需要看当前弈子数量，不自动选择",
      rank: "A",
      score: "4.72",
    },
    {
      title: "利滚利",
      body: "经济备选项，等待用户决策",
      rank: "B",
      score: "4.91",
    },
  ]);
  const [simulator, setSimulator] = useState({
    championName: "Ahri",
    level: 7,
    panelExpanded: true,
    selectedSlot: "",
  });
  const [runtimePanel, setRuntimePanel] = useState({
    panelExpanded: false,
    slot1: "",
    slot2: "",
    slot3: "",
    selectedSlot: "",
  });
  const [apexQuery, setApexQuery] = useState({
    championName: "放逐之刃",
    augmentName: "灵魂虹吸",
  });
  const [error, setError] = useState<ErrorState | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  useEffect(() => {
    void loadOverview();
    void loadRuntimeStatus();
    void loadMonitors();
  }, []);

  useEffect(() => {
    if (!runtimeLoop?.listening) {
      return;
    }

    const timer = window.setInterval(() => {
      void loadRuntimeStatus(true);
    }, 2500);
    return () => window.clearInterval(timer);
  }, [runtimeLoop?.listening]);

  const directoryReadyCount = useMemo(
    () => overview?.directories.filter((item) => item.exists).length ?? 0,
    [overview],
  );

  async function runCommand<T>(
    key: string,
    command: string,
    args?: Record<string, unknown>,
  ): Promise<T | null> {
    setBusy(key);
    setError(null);
    try {
      return await invoke<T>(command, args);
    } catch (caught) {
      setError(extractErrorState(caught));
      return null;
    } finally {
      setBusy(null);
    }
  }

  async function loadOverview() {
    const data = await runCommand<RuntimeOverview>("overview", "get_runtime_overview");
    if (data) {
      setOverview(data);
    }
  }

  async function loadRuntimeStatus(silent = false) {
    if (silent) {
      try {
        setRuntimeLoop(await invoke<RuntimeLoopSnapshot>("get_runtime_orchestrator_status"));
      } catch (caught) {
        setError(extractErrorState(caught));
      }
      return;
    }

    const data = await runCommand<RuntimeLoopSnapshot>(
      "runtime-status",
      "get_runtime_orchestrator_status",
    );
    if (data) {
      setRuntimeLoop(data);
    }
  }

  async function loadMonitors() {
    const data = await runCommand<MonitorDiagnostic[]>("monitors", "list_capture_monitors");
    if (data) {
      setMonitors(data);
      const primary = data.find((monitor) => monitor.primary) ?? data[0];
      if (primary && selectedMonitorId === "") {
        setSelectedMonitorId(String(primary.id));
      }
    }
  }

  async function runHealthCheck() {
    const data = await runCommand<HealthCheckReport>("health", "run_health_check");
    if (data) {
      setHealth(data);
      await loadOverview();
    }
  }

  async function exportDiagnostics() {
    const data = await runCommand<DiagnosticExportResult>("diagnostic-export", "export_diagnostic_package");
    if (data) {
      setDiagnosticExport(data);
      await loadOverview();
    }
  }

  async function exportRelease() {
    const data = await runCommand<DiagnosticExportResult>("release-export", "export_release_package");
    if (data) {
      setReleaseExport(data);
      await loadOverview();
    }
  }

  async function captureSample() {
    const preferredMonitorId = selectedMonitorId === "" ? null : Number.parseInt(selectedMonitorId, 10);
    const data = await runCommand<CaptureSampleReport>("capture", "capture_monitor_sample", {
      preferredMonitorId,
    });
    if (data) {
      setCaptureReport(data);
      setCalibrationForm((current) => ({
        ...current,
        screenshotWidth: String(data.image.width),
        screenshotHeight: String(data.image.height),
      }));
    }
  }

  function buildCalibrationInput() {
    const screenshotSize = {
      width: parseUint(calibrationForm.screenshotWidth, "原始截图宽度"),
      height: parseUint(calibrationForm.screenshotHeight, "原始截图高度"),
    };
    return {
      input: {
        screenshotSize,
        nameRegions: calibrationForm.nameRegions.map((rect, index) =>
          rectFromFields(rect, nameRegionLabels[index]),
        ),
        bottomAnchors: calibrationForm.bottomAnchors.map((point, index) =>
          pointFromFields(point, anchorLabels[index]),
        ),
        bottomButtonRegion: rectFromFields(calibrationForm.bottomButtonRegion, "底部按钮区域"),
      },
    };
  }

  async function saveCalibration() {
    let args: ReturnType<typeof buildCalibrationInput>;
    try {
      args = buildCalibrationInput();
    } catch (caught) {
      setError(extractErrorState(caught));
      return;
    }
    const data = await runCommand<CalibrationProfileResult>(
      "calibration-save",
      "save_pixel_calibration_profile",
      args,
    );
    if (data) {
      setCalibrationResult(data);
      setCalibrationForm(calibrationFormFromEcho(data));
      await loadOverview();
    }
  }

  async function loadCalibration() {
    const data = await runCommand<CalibrationProfileResult>(
      "calibration-load",
      "load_calibration_profile",
    );
    if (data) {
      setCalibrationResult(data);
      setCalibrationForm(calibrationFormFromEcho(data));
    }
  }

  function updateNameRegion(index: number, field: keyof RectFields, value: string) {
    setCalibrationForm((current) => {
      const nameRegions = [...current.nameRegions] as [RectFields, RectFields, RectFields];
      nameRegions[index] = { ...nameRegions[index], [field]: value };
      return { ...current, nameRegions };
    });
  }

  function updateBottomAnchor(index: number, field: keyof PointFields, value: string) {
    setCalibrationForm((current) => {
      const bottomAnchors = [...current.bottomAnchors] as [PointFields, PointFields, PointFields];
      bottomAnchors[index] = { ...bottomAnchors[index], [field]: value };
      return { ...current, bottomAnchors };
    });
  }

  function updateBottomButtonRegion(field: keyof RectFields, value: string) {
    setCalibrationForm((current) => ({
      ...current,
      bottomButtonRegion: { ...current.bottomButtonRegion, [field]: value },
    }));
  }

  async function checkOcrResources() {
    const data = await runCommand<OcrResourceStatus>("ocr-status", "check_ocr_resources");
    if (data) {
      setOcrStatus(data);
    }
  }

  async function replayOcrText() {
    const data = await runCommand<OfflineReplayReport>("ocr-replay", "run_ocr_text_replay", {
      input: {
        ...ocrTexts,
        confidence: 0.95,
      },
    });
    if (data) {
      setOcrReplay(data);
    }
  }

  async function fetchLiveClient() {
    const data = await runCommand<ActivePlayerSnapshot>(
      "live-client",
      "fetch_live_client_active_player",
    );
    if (data) {
      setLiveClient(data);
    }
  }

  async function simulateStateMachine() {
    const selectedSlot =
      simulator.selectedSlot.trim() === "" ? null : Number.parseInt(simulator.selectedSlot, 10);
    const data = await runCommand<StateMachineResult>("state-machine", "evaluate_state_machine", {
      input: {
        player: {
          championName: simulator.championName,
          level: simulator.level,
        },
        panelState: simulator.panelExpanded ? "expanded" : "collapsed",
        choices: [
          { slot: 0, augmentId: "prismatic-ticket" },
          { slot: 1, augmentId: "build-a-bud" },
          { slot: 2, augmentId: "trade-sector" },
        ],
        selectedSlot,
        pauseReason: null,
      },
    });
    if (data) {
      setStateResult(data);
    }
  }

  function buildRuntimeRequest() {
    const choices = [
      { slot: 1, augmentId: runtimePanel.slot1.trim() },
      { slot: 2, augmentId: runtimePanel.slot2.trim() },
      { slot: 3, augmentId: runtimePanel.slot3.trim() },
    ].filter((choice) => choice.augmentId.length > 0);
    const parsedSelectedSlot = Number.parseInt(runtimePanel.selectedSlot, 10);
    const selectedSlot =
      runtimePanel.selectedSlot.trim() === "" || !Number.isFinite(parsedSelectedSlot)
        ? null
        : parsedSelectedSlot;

    return {
      request: {
        panelSnapshot: {
          panelState: runtimePanel.panelExpanded ? "expanded" : "collapsed",
          choices,
          selectedSlot,
        },
      },
    };
  }

  async function triggerRuntimeLoop() {
    const data = await runCommand<RuntimeLoopSnapshot>(
      "runtime-trigger",
      "trigger_runtime_orchestrator",
      buildRuntimeRequest(),
    );
    if (data) {
      setRuntimeLoop(data);
    }
  }

  async function startRuntimeListener() {
    const data = await runCommand<RuntimeLoopSnapshot>(
      "runtime-start",
      "start_runtime_listener",
      buildRuntimeRequest(),
    );
    if (data) {
      setRuntimeLoop(data);
    }
  }

  async function stopRuntimeListener() {
    const data = await runCommand<RuntimeLoopSnapshot>(
      "runtime-stop",
      "stop_runtime_listener",
    );
    if (data) {
      setRuntimeLoop(data);
    }
  }

  async function loadApexCacheReport() {
    const data = await runCommand<ApexCacheReport>("apex-cache", "build_apex_cache_report");
    if (data) {
      setApexReport(data);
    }
  }

  async function lookupApex(forceRefresh: boolean) {
    const data = await runCommand<ApexLookupResult>("apex-lookup", "lookup_apex_lol", {
      request: {
        championName: apexQuery.championName.trim(),
        augmentName: apexQuery.augmentName.trim(),
        forceRefresh,
      },
    });
    if (data) {
      setApexResult(data);
      await loadApexCacheReport();
    }
  }

  async function showOverlayCard() {
    const data = await runCommand<OverlayOperationReport>("overlay-show", "show_overlay_test_card", {
      request: {
        monitorName: null,
        anchor: "bottomRight",
        width: 260,
        height: 118,
        gap: overview?.settings.overlay.gap ?? 18,
        clickThrough: overview?.settings.overlay.clickThrough ?? true,
      },
    });
    if (data) {
      setOverlayReport(data);
    }
  }

  async function hideOverlayCard() {
    const data = await runCommand<OverlayOperationReport>("overlay-hide", "hide_overlay_test_card");
    if (data) {
      setOverlayReport(data);
    }
  }

  async function updateOverlaySlots() {
    const slots = overlaySlots.map((slot, index) => ({
      slot: index + 1,
      title: slot.title,
      body: slot.body,
      rank: slot.rank,
      score: slot.score,
      augmentId: `manual-slot-${index + 1}`,
    }));
    const data = await runCommand<OverlaySlotUpdateReport>("overlay-update", "update_overlay_slots", {
      slots,
    });
    if (data) {
      setOverlayUpdateReport(data);
    }
  }

  return (
    <main className="app-shell">
      <header className="topbar">
        <div>
          <p className="eyebrow">LOL Hex Assistant</p>
          <h1>诊断工作台</h1>
        </div>
        <div className="actions">
          <button type="button" onClick={runHealthCheck} disabled={busy !== null}>
            健康检查
          </button>
          <button type="button" onClick={exportDiagnostics} disabled={busy !== null}>
            导出诊断包
          </button>
          <button type="button" onClick={exportRelease} disabled={busy !== null}>
            导出 release
          </button>
        </div>
      </header>

      {error ? (
        <section className="error-banner">
          错误码：{error.code}；{error.message}
        </section>
      ) : null}

      <section className="summary-grid" aria-label="运行概览">
        <article className="panel">
          <h2>基础设施</h2>
          <dl className="metric-list">
            <div>
              <dt>应用数据目录</dt>
              <dd>{overview?.appDataDir ?? "加载中"}</dd>
            </div>
            <div>
              <dt>目录准备</dt>
              <dd>
                {directoryReadyCount}/{overview?.directories.length ?? 0}
              </dd>
            </div>
            <div>
              <dt>日志文件</dt>
              <dd>{overview?.latestLogPath ?? "加载中"}</dd>
            </div>
          </dl>
        </article>

        <article className="panel">
          <h2>运行参数</h2>
          <dl className="metric-list compact">
            <div>
              <dt>显示模式</dt>
              <dd>{overview?.settings.capture.defaultDisplayMode ?? "无边框"}</dd>
            </div>
            <div>
              <dt>OCR 引擎</dt>
              <dd>{overview?.settings.ocr.engine ?? "待加载"}</dd>
            </div>
            <div>
              <dt>Overlay</dt>
              <dd>{overview?.settings.overlay.enabled ? "启用" : "关闭"}</dd>
            </div>
            <div>
              <dt>ApexLOL 超时</dt>
              <dd>{overview ? `${overview.settings.apexLol.requestTimeoutMs} ms` : "待加载"}</dd>
            </div>
            <div>
              <dt>ApexLOL TTL</dt>
              <dd>
                {overview
                  ? `${overview.settings.apexLol.cacheTtlHours} 小时 / 失败 ${overview.settings.apexLol.failedCacheTtlMinutes} 分钟`
                  : "待加载"}
              </dd>
            </div>
          </dl>
        </article>
      </section>

      <section className="panel">
        <h2>健康检查</h2>
        {health ? (
          <>
            <p className="trace-line">
              trace id：{health.traceId}，生成时间：{health.generatedAt}
            </p>
            <div className="health-list">
              {health.items.map((item) => (
                <article key={item.key} className="health-row">
                  <span className={`badge ${statusClass[item.status]}`}>{statusText[item.status]}</span>
                  <div>
                    <h3>{item.name}</h3>
                    <p>{item.details}</p>
                    {item.errorCode ? <small>错误码：{item.errorCode}</small> : null}
                  </div>
                </article>
              ))}
            </div>
          </>
        ) : (
          <p className="empty-state">暂无健康检查报告。</p>
        )}
      </section>

      <section className="workbench-grid">
        <article className="panel">
          <div className="panel-heading">
            <h2>截图诊断</h2>
            <div className="inline-actions">
              <button type="button" onClick={loadMonitors} disabled={busy !== null}>
                刷新显示器
              </button>
              <button type="button" onClick={captureSample} disabled={busy !== null}>
                采集样本
              </button>
            </div>
          </div>
          <label className="field-stack">
            目标显示器
            <select
              value={selectedMonitorId}
              onChange={(event) => setSelectedMonitorId(event.target.value)}
              disabled={busy !== null || monitors.length === 0}
            >
              <option value="">主显示器</option>
              {monitors.map((monitor) => (
                <option key={monitor.id} value={monitor.id}>
                  {monitor.primary ? "主屏 · " : ""}
                  {monitor.friendlyName || monitor.name || `显示器 ${monitor.id}`} · {monitor.width}x
                  {monitor.height} @ {monitor.x},{monitor.y}
                </option>
              ))}
            </select>
          </label>
          {captureReport ? (
            <dl className="metric-list">
              <div>
                <dt>显示器</dt>
                <dd>
                  {captureReport.monitor.friendlyName || captureReport.monitor.name} ·{" "}
                  {captureReport.monitor.width}x{captureReport.monitor.height}
                </dd>
              </div>
              <div>
                <dt>画面质量</dt>
                <dd>
                  亮度 {captureReport.image.meanLuma.toFixed(2)} ·{" "}
                  范围 {captureReport.image.minLuma}-{captureReport.image.maxLuma} ·{" "}
                  {captureReport.image.blackScreen ? "黑屏" : "可见"} ·{" "}
                  {captureReport.staleFrame ? "重复帧" : "新帧"}
                </dd>
              </div>
              <div>
                <dt>尺寸与耗时</dt>
                <dd>
                  {captureReport.image.width}x{captureReport.image.height} · 截图{" "}
                  {captureReport.image.captureDurationMs} ms · 保存{" "}
                  {captureReport.image.saveDurationMs} ms
                </dd>
              </div>
              <div>
                <dt>样本 PNG</dt>
                <dd>{captureReport.pngPath}</dd>
              </div>
              <div>
                <dt>诊断 JSON</dt>
                <dd>{captureReport.jsonPath}</dd>
              </div>
              <div>
                <dt>帧诊断</dt>
                <dd>
                  高亮像素 {captureReport.image.brightPixelRatio.toFixed(6)} · hash{" "}
                  {captureReport.image.frameHash.slice(0, 16)}
                  {captureReport.previousFrameHash
                    ? ` · 旧帧 ${captureReport.previousFrameHash.slice(0, 16)}`
                    : ""}
                </dd>
              </div>
            </dl>
          ) : (
            <p className="empty-state">暂无截图样本。</p>
          )}
        </article>

        <article className="panel wide-panel">
          <div className="panel-heading">
            <h2>用户校准</h2>
            <div className="inline-actions">
              <button type="button" onClick={loadCalibration} disabled={busy !== null}>
                加载配置
              </button>
              <button type="button" onClick={saveCalibration} disabled={busy !== null}>
                保存配置
              </button>
            </div>
          </div>
          <div className="form-grid size-grid">
            <label>
              原始截图宽度
              <input
                inputMode="numeric"
                value={calibrationForm.screenshotWidth}
                onChange={(event) =>
                  setCalibrationForm({ ...calibrationForm, screenshotWidth: event.target.value })
                }
              />
            </label>
            <label>
              原始截图高度
              <input
                inputMode="numeric"
                value={calibrationForm.screenshotHeight}
                onChange={(event) =>
                  setCalibrationForm({ ...calibrationForm, screenshotHeight: event.target.value })
                }
              />
            </label>
          </div>
          <div className="calibration-grid">
            {calibrationForm.nameRegions.map((rect, index) => (
              <fieldset key={nameRegionLabels[index]} className="calibration-group">
                <legend>{nameRegionLabels[index]}</legend>
                <input
                  aria-label={`${nameRegionLabels[index]} X`}
                  inputMode="numeric"
                  value={rect.x}
                  onChange={(event) => updateNameRegion(index, "x", event.target.value)}
                />
                <input
                  aria-label={`${nameRegionLabels[index]} Y`}
                  inputMode="numeric"
                  value={rect.y}
                  onChange={(event) => updateNameRegion(index, "y", event.target.value)}
                />
                <input
                  aria-label={`${nameRegionLabels[index]} 宽度`}
                  inputMode="numeric"
                  value={rect.width}
                  onChange={(event) => updateNameRegion(index, "width", event.target.value)}
                />
                <input
                  aria-label={`${nameRegionLabels[index]} 高度`}
                  inputMode="numeric"
                  value={rect.height}
                  onChange={(event) => updateNameRegion(index, "height", event.target.value)}
                />
              </fieldset>
            ))}
            {calibrationForm.bottomAnchors.map((point, index) => (
              <fieldset key={anchorLabels[index]} className="calibration-group point-group">
                <legend>{anchorLabels[index]}</legend>
                <input
                  aria-label={`${anchorLabels[index]} X`}
                  inputMode="numeric"
                  value={point.x}
                  onChange={(event) => updateBottomAnchor(index, "x", event.target.value)}
                />
                <input
                  aria-label={`${anchorLabels[index]} Y`}
                  inputMode="numeric"
                  value={point.y}
                  onChange={(event) => updateBottomAnchor(index, "y", event.target.value)}
                />
              </fieldset>
            ))}
            <fieldset className="calibration-group">
              <legend>底部按钮区域</legend>
              <input
                aria-label="底部按钮区域 X"
                inputMode="numeric"
                value={calibrationForm.bottomButtonRegion.x}
                onChange={(event) => updateBottomButtonRegion("x", event.target.value)}
              />
              <input
                aria-label="底部按钮区域 Y"
                inputMode="numeric"
                value={calibrationForm.bottomButtonRegion.y}
                onChange={(event) => updateBottomButtonRegion("y", event.target.value)}
              />
              <input
                aria-label="底部按钮区域宽度"
                inputMode="numeric"
                value={calibrationForm.bottomButtonRegion.width}
                onChange={(event) => updateBottomButtonRegion("width", event.target.value)}
              />
              <input
                aria-label="底部按钮区域高度"
                inputMode="numeric"
                value={calibrationForm.bottomButtonRegion.height}
                onChange={(event) => updateBottomButtonRegion("height", event.target.value)}
              />
            </fieldset>
          </div>
          {calibrationResult ? (
            <dl className="metric-list">
              <div>
                <dt>配置路径</dt>
                <dd>{calibrationResult.path}</dd>
              </div>
              <div>
                <dt>原始截图尺寸</dt>
                <dd>
                  {calibrationResult.echo.screenshotSize.width}x
                  {calibrationResult.echo.screenshotSize.height}
                </dd>
              </div>
              <div>
                <dt>归一化坐标回显</dt>
                <dd>
                  名称 {calibrationResult.echo.nameRegions.map(formatRect).join(" / ")}；锚点{" "}
                  {calibrationResult.echo.bottomAnchors.map(formatPoint).join(" / ")}；按钮{" "}
                  {formatRect(calibrationResult.echo.bottomButtonRegion)}
                </dd>
              </div>
            </dl>
          ) : (
            <p className="empty-state">暂无校准配置回显。</p>
          )}
        </article>

        <article className="panel">
          <div className="panel-heading">
            <h2>OCR 资源</h2>
            <button type="button" onClick={checkOcrResources} disabled={busy !== null}>
              检查资源
            </button>
          </div>
          {ocrStatus ? (
            <dl className="metric-list">
              <div>
                <dt>模型状态</dt>
                <dd>{ocrStatus.ready ? "已就绪" : "缺失"}</dd>
              </div>
              <div>
                <dt>模型路径</dt>
                <dd>{ocrStatus.modelPath}</dd>
              </div>
              <div>
                <dt>说明</dt>
                <dd>{ocrStatus.message}</dd>
              </div>
            </dl>
          ) : (
            <p className="empty-state">暂无 OCR 资源报告。</p>
          )}
        </article>

        <article className="panel">
          <div className="panel-heading">
            <h2>OCR 文本回放</h2>
            <button type="button" onClick={replayOcrText} disabled={busy !== null}>
              回放
            </button>
          </div>
          <div className="form-grid">
            <label>
              左侧
              <input
                value={ocrTexts.leftText}
                onChange={(event) => setOcrTexts({ ...ocrTexts, leftText: event.target.value })}
              />
            </label>
            <label>
              中间
              <input
                value={ocrTexts.centerText}
                onChange={(event) => setOcrTexts({ ...ocrTexts, centerText: event.target.value })}
              />
            </label>
            <label>
              右侧
              <input
                value={ocrTexts.rightText}
                onChange={(event) => setOcrTexts({ ...ocrTexts, rightText: event.target.value })}
              />
            </label>
          </div>
          {ocrReplay ? (
            <div className="result-list">
              {ocrReplay.slots.map((slot) => (
                <div key={slot.slot} className="result-row">
                  <strong>{replaySlotLabel[slot.slot]}</strong>
                  <span>{slot.finalName ?? "未匹配"}</span>
                  <small>
                    分数 {slot.matchScore.toFixed(3)}
                    {slot.failureReason ? ` · ${slot.failureReason}` : ""}
                  </small>
                </div>
              ))}
            </div>
          ) : null}
        </article>

        <article className="panel">
          <div className="panel-heading">
            <h2>Live Client</h2>
            <button type="button" onClick={fetchLiveClient} disabled={busy !== null}>
              读取
            </button>
          </div>
          {liveClient ? (
            <dl className="metric-list compact">
              <div>
                <dt>英雄</dt>
                <dd>{liveClient.championName}</dd>
              </div>
              <div>
                <dt>等级</dt>
                <dd>{liveClient.level}</dd>
              </div>
            </dl>
          ) : (
            <p className="empty-state">暂无 Live Client 数据。</p>
          )}
        </article>

        <article className="panel runtime-panel">
          <div className="panel-heading">
            <h2>局内闭环编排</h2>
            <div className="inline-actions">
              <button type="button" onClick={triggerRuntimeLoop} disabled={busy !== null}>
                手动触发
              </button>
              <button
                type="button"
                onClick={startRuntimeListener}
                disabled={busy !== null || runtimeLoop?.listening === true}
              >
                启动监听
              </button>
              <button
                type="button"
                onClick={stopRuntimeListener}
                disabled={busy !== null || runtimeLoop?.listening !== true}
              >
                停止监听
              </button>
            </div>
          </div>
          <div className="form-grid">
            <label className="checkbox-label">
              <input
                type="checkbox"
                checked={runtimePanel.panelExpanded}
                onChange={(event) =>
                  setRuntimePanel({ ...runtimePanel, panelExpanded: event.target.checked })
                }
              />
              面板展开
            </label>
            <label>
              左侧 slot
              <input
                value={runtimePanel.slot1}
                onChange={(event) => setRuntimePanel({ ...runtimePanel, slot1: event.target.value })}
              />
            </label>
            <label>
              中间 slot
              <input
                value={runtimePanel.slot2}
                onChange={(event) => setRuntimePanel({ ...runtimePanel, slot2: event.target.value })}
              />
            </label>
            <label>
              右侧 slot
              <input
                value={runtimePanel.slot3}
                onChange={(event) => setRuntimePanel({ ...runtimePanel, slot3: event.target.value })}
              />
            </label>
            <label>
              已选 slot
              <input
                inputMode="numeric"
                value={runtimePanel.selectedSlot}
                onChange={(event) =>
                  setRuntimePanel({ ...runtimePanel, selectedSlot: event.target.value })
                }
              />
            </label>
          </div>
          {runtimeLoop ? (
            <>
              <dl className="metric-list compact">
                <div>
                  <dt>监听状态</dt>
                  <dd>{runtimeLoop.listening ? "运行中" : "已停止"}</dd>
                </div>
                <div>
                  <dt>状态机</dt>
                  <dd>{assistantStatusText[runtimeLoop.state.status] ?? runtimeLoop.state.status}</dd>
                </div>
                <div>
                  <dt>英雄/等级</dt>
                  <dd>
                    {runtimeLoop.state.player
                      ? `${runtimeLoop.state.player.championName} / ${runtimeLoop.state.player.level}`
                      : "无 Live Client 数据"}
                  </dd>
                </div>
                <div>
                  <dt>待处理档位</dt>
                  <dd>
                    {runtimeLoop.state.pendingTiers.length > 0
                      ? runtimeLoop.state.pendingTiers.join(" / ")
                      : "无"}
                  </dd>
                </div>
                <div>
                  <dt>面板状态</dt>
                  <dd>{runtimeLoop.state.panelState === "expanded" ? "展开" : "收起"}</dd>
                </div>
                <div>
                  <dt>错误码</dt>
                  <dd>{runtimeLoop.lastErrorCode ?? runtimeLoop.state.pauseReason ?? "无"}</dd>
                </div>
              </dl>
              <div className="event-list">
                {runtimeLoop.recentEvents.slice(0, 5).map((event) => (
                  <article key={event.traceId} className="event-row">
                    <strong>{runtimeTriggerText[event.triggerEvent] ?? event.triggerEvent}</strong>
                    <span>{event.errorCode ?? "OK"}</span>
                    <p>
                      {event.championName ?? "未知英雄"} · {event.level ?? "无等级"} · 待处理{" "}
                      {event.pendingTiers.length > 0 ? event.pendingTiers.join("/") : "无"} · slot{" "}
                      {event.slotChanges.length}
                    </p>
                    <small>
                      trace id：{event.traceId} · {event.occurredAt}
                    </small>
                  </article>
                ))}
              </div>
            </>
          ) : (
            <p className="empty-state">暂无编排状态。</p>
          )}
        </article>

        <article className="panel">
          <div className="panel-heading">
            <h2>状态机模拟</h2>
            <button type="button" onClick={simulateStateMachine} disabled={busy !== null}>
              模拟
            </button>
          </div>
          <div className="form-grid">
            <label>
              英雄
              <input
                value={simulator.championName}
                onChange={(event) => setSimulator({ ...simulator, championName: event.target.value })}
              />
            </label>
            <label>
              等级
              <input
                type="number"
                min="1"
                max="18"
                value={simulator.level}
                onChange={(event) =>
                  setSimulator({ ...simulator, level: Number.parseInt(event.target.value, 10) || 1 })
                }
              />
            </label>
            <label>
              已选槽位
              <input
                inputMode="numeric"
                value={simulator.selectedSlot}
                onChange={(event) => setSimulator({ ...simulator, selectedSlot: event.target.value })}
              />
            </label>
            <label className="checkbox-label">
              <input
                type="checkbox"
                checked={simulator.panelExpanded}
                onChange={(event) => setSimulator({ ...simulator, panelExpanded: event.target.checked })}
              />
              面板展开
            </label>
          </div>
          {stateResult ? (
            <dl className="metric-list compact">
              <div>
                <dt>状态</dt>
                <dd>{stateResult.state.status}</dd>
              </div>
              <div>
                <dt>待选阶段</dt>
                <dd>{stateResult.state.pendingTier ?? "无"}</dd>
              </div>
              <div>
                <dt>事件数</dt>
                <dd>{stateResult.events.length}</dd>
              </div>
              <div>
                <dt>可见槽位</dt>
                <dd>{Object.keys(stateResult.state.visibleChoices).length}</dd>
              </div>
            </dl>
          ) : null}
        </article>

        <article className="panel">
          <div className="panel-heading">
            <h2>ApexLOL 查询</h2>
            <div className="inline-actions">
              <button
                type="button"
                onClick={() => void lookupApex(false)}
                disabled={busy !== null || !apexQuery.championName.trim() || !apexQuery.augmentName.trim()}
              >
                查询
              </button>
              <button
                type="button"
                onClick={() => void lookupApex(true)}
                disabled={busy !== null || !apexQuery.championName.trim() || !apexQuery.augmentName.trim()}
              >
                刷新
              </button>
            </div>
          </div>
          <div className="form-grid two-columns">
            <label>
              英雄
              <input
                value={apexQuery.championName}
                onChange={(event) =>
                  setApexQuery({ ...apexQuery, championName: event.target.value })
                }
              />
            </label>
            <label>
              海克斯
              <input
                value={apexQuery.augmentName}
                onChange={(event) =>
                  setApexQuery({ ...apexQuery, augmentName: event.target.value })
                }
              />
            </label>
          </div>
          {apexResult ? (
            <div className="apex-result">
              <div className="apex-result-title">
                <span className={`badge ${apexStatusClass[apexResult.status]}`}>
                  {apexStatusText[apexResult.status]}
                </span>
                <strong>
                  {apexResult.championName} · {apexResult.augmentName}
                </strong>
              </div>
              {apexResult.status === "ok" ? (
                <dl className="metric-list">
                  <div>
                    <dt>评级</dt>
                    <dd className="apex-rating">{apexResult.rating ?? "暂无数据"}</dd>
                  </div>
                  <div>
                    <dt>摘要</dt>
                    <dd>{apexResult.summary}</dd>
                  </div>
                  <div>
                    <dt>提醒</dt>
                    <dd>{apexResult.tip ?? "暂无数据"}</dd>
                  </div>
                </dl>
              ) : (
                <dl className="metric-list">
                  <div>
                    <dt>结果</dt>
                    <dd>暂无数据</dd>
                  </div>
                  <div>
                    <dt>失败原因</dt>
                    <dd>{apexResult.error ?? apexResult.requestLog.failureReason ?? "未返回原因"}</dd>
                  </div>
                </dl>
              )}
              <dl className="metric-list compact apex-meta">
                <div>
                  <dt>缓存</dt>
                  <dd>{apexResult.cacheHit ? "命中" : "未命中"}</dd>
                </div>
                <div>
                  <dt>耗时</dt>
                  <dd>{apexResult.requestLog.durationMs} ms</dd>
                </div>
                <div>
                  <dt>获取时间</dt>
                  <dd>{apexResult.fetchedAt}</dd>
                </div>
                <div>
                  <dt>过期时间</dt>
                  <dd>{apexResult.expiresAt}</dd>
                </div>
                <div>
                  <dt>来源 URL</dt>
                  <dd>{apexResult.sourceUrl}</dd>
                </div>
                <div>
                  <dt>缓存键</dt>
                  <dd>{apexResult.cacheKey}</dd>
                </div>
              </dl>
            </div>
          ) : (
            <p className="empty-state">输入英雄和海克斯后可查询；刷新会绕过本地缓存。</p>
          )}
        </article>

        <article className="panel">
          <div className="panel-heading">
            <h2>ApexLOL 缓存报告</h2>
            <button type="button" onClick={loadApexCacheReport} disabled={busy !== null}>
              生成报告
            </button>
          </div>
          {apexReport ? (
            <>
              <dl className="metric-list compact">
                <div>
                  <dt>总条目</dt>
                  <dd>{apexReport.totalEntries}</dd>
                </div>
                <div>
                  <dt>成功/失败/过期</dt>
                  <dd>
                    {apexReport.okEntries}/{apexReport.failedEntries}/{apexReport.expiredEntries}
                  </dd>
                </div>
              </dl>
              <p className="trace-line">
                缓存：{apexReport.cachePath}
                {apexReport.reportPath ? `；报告：${apexReport.reportPath}` : ""}
              </p>
              {apexReport.entries.length > 0 ? (
                <div className="cache-entry-list">
                  {apexReport.entries.slice(0, 6).map((entry) => (
                    <article key={entry.cacheKey} className="cache-entry">
                      <div>
                        <strong>
                          {entry.championName} · {entry.augmentName}
                        </strong>
                        <span className={`badge ${apexStatusClass[entry.status]}`}>
                          {apexStatusText[entry.status]}
                        </span>
                      </div>
                      <p>
                        {entry.rating ? `评级 ${entry.rating} · ` : ""}
                        {entry.summary}
                      </p>
                      <small>
                        {entry.expired ? "已过期" : "有效"} · {entry.durationMs} ms ·{" "}
                        {entry.fetchedAt} · {entry.sourceUrl}
                        {entry.error ? ` · ${entry.error}` : ""}
                      </small>
                    </article>
                  ))}
                </div>
              ) : null}
            </>
          ) : (
            <p className="empty-state">暂无 ApexLOL 缓存报告。</p>
          )}
        </article>

        <article className="panel">
          <div className="panel-heading">
            <h2>Overlay 测试卡片</h2>
            <div className="inline-actions">
              <button type="button" onClick={showOverlayCard} disabled={busy !== null}>
                显示
              </button>
              <button type="button" onClick={hideOverlayCard} disabled={busy !== null}>
                隐藏
              </button>
            </div>
          </div>
          <div className="overlay-slot-form">
            {overlaySlots.map((slot, index) => (
              <label key={index}>
                {index + 1} 号卡片
                <input
                  value={slot.title}
                  onChange={(event) => {
                    const next = [...overlaySlots];
                    next[index] = { ...slot, title: event.target.value };
                    setOverlaySlots(next);
                  }}
                />
                <input
                  value={slot.body}
                  onChange={(event) => {
                    const next = [...overlaySlots];
                    next[index] = { ...slot, body: event.target.value };
                    setOverlaySlots(next);
                  }}
                />
              </label>
            ))}
          </div>
          <button type="button" onClick={updateOverlaySlots} disabled={busy !== null}>
            更新真实 slot 数据
          </button>
          {overlayReport ? (
            <dl className="metric-list">
              <div>
                <dt>可见状态</dt>
                <dd>{overlayReport.visible ? "可见" : "隐藏"}</dd>
              </div>
              <div>
                <dt>Overlay 区域</dt>
                <dd>
                  {overlayReport.bounds.x}, {overlayReport.bounds.y},{" "}
                  {overlayReport.bounds.width}x{overlayReport.bounds.height}
                </dd>
              </div>
              <div>
                <dt>目标显示器</dt>
                <dd>
                  {overlayReport.monitor.name ?? "主显示器"} · {overlayReport.monitor.size.width}x
                  {overlayReport.monitor.size.height} · scale {overlayReport.monitor.scaleFactor}
                </dd>
              </div>
              <div>
                <dt>卡片数量</dt>
                <dd>
                  {overlayReport.cards.length} 张 ·{" "}
                  {overlayReport.cards.map((card) => `${card.slot}:${card.source}`).join(" / ")}
                </dd>
              </div>
              <div>
                <dt>点击穿透</dt>
                <dd>
                  {overlayReport.clickThrough.status} · {overlayReport.clickThrough.message}
                </dd>
              </div>
              <div>
                <dt>日志</dt>
                <dd>{overlayReport.logPath ?? "未返回日志路径"}</dd>
              </div>
            </dl>
          ) : (
            <p className="empty-state">暂无 Overlay 报告。</p>
          )}
          {overlayUpdateReport ? (
            <p className="trace-line">
              {overlayUpdateReport.message} {overlayUpdateReport.logPath ?? ""}
            </p>
          ) : null}
        </article>

        <article className="panel">
          <h2>导出结果</h2>
          <dl className="metric-list">
            <div>
              <dt>诊断包</dt>
              <dd>
                {diagnosticExport
                  ? `${diagnosticExport.zipPath} · ${diagnosticExport.includedFiles} 个文件`
                  : "未导出"}
              </dd>
            </div>
            <div>
              <dt>release</dt>
              <dd>
                {releaseExport
                  ? `${releaseExport.zipPath} · ${releaseExport.includedFiles} 个文件`
                  : "未导出"}
              </dd>
            </div>
          </dl>
        </article>
      </section>

      <section className="summary-grid">
        <article className="panel">
          <h2>应用数据子目录</h2>
          <div className="directory-grid">
            {overview?.directories.map((item) => (
              <div key={item.key} className="directory-item">
                <span className={item.exists ? "dot ready" : "dot"} />
                <div>
                  <strong>{item.key}</strong>
                  <p>{item.path}</p>
                </div>
              </div>
            ))}
          </div>
        </article>

        <article className="panel">
          <h2>安全边界</h2>
          <div className="boundary-list">
            {safeBoundaries.map((item) => (
              <span key={item}>{item}</span>
            ))}
          </div>
        </article>
      </section>
    </main>
  );
}

function OverlayPage() {
  const [payload, setPayload] = useState<OverlayPagePayload>(() => {
    return (
      window.__HEX_OVERLAY_BOOTSTRAP__ ?? {
        generatedAt: new Date().toISOString(),
        mode: "fallback",
        cards: fallbackOverlayCards(),
      }
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
        cards: current.cards.map((card) => {
          const next = customEvent.detail.find((slot) => slot.slot === card.slot);
          return next
            ? {
                ...card,
                title: next.title,
                body: next.body ?? card.body,
                augmentId: next.augmentId ?? card.augmentId,
                rank: next.rank ?? card.rank,
                score: next.score ?? card.score,
              }
            : card;
        }),
      }));
    }
    window.addEventListener("hex-overlay-slots", handleSlotUpdate);
    return () => {
      document.documentElement.classList.remove("overlay-document");
      window.removeEventListener("hex-overlay-slots", handleSlotUpdate);
    };
  }, []);

  return (
    <main className="overlay-root" aria-label="Overlay 测试卡片">
      {payload.cards.map((card) => (
        <article
          key={card.slot}
          className="overlay-card"
          style={{
            left: `${card.bounds.x}px`,
            top: `${card.bounds.y}px`,
            width: `${card.bounds.width}px`,
            height: `${card.bounds.height}px`,
          }}
        >
          <div className="overlay-card-topline">
            <span>Slot {card.slot}</span>
            {card.rank ? <strong>{card.rank}</strong> : null}
          </div>
          <h1>{card.title}</h1>
          <p>{card.body}</p>
          <footer>
            <span>{card.score ? `均分 ${card.score}` : payload.mode === "static" ? "静态测试" : "实时数据"}</span>
            <span>{card.augmentId ?? card.source}</span>
          </footer>
        </article>
      ))}
    </main>
  );
}

function fallbackOverlayCards(): OverlayCardInfo[] {
  return [1, 2, 3].map((slot, index) => ({
    slot,
    title: `测试卡片 ${slot}`,
    body: "Overlay 页面未收到后端初始数据",
    augmentId: null,
    rank: null,
    score: null,
    bounds: {
      x: 80 + index * 280,
      y: 120,
      width: 260,
      height: 118,
    },
    source: "frontend.fallback",
  }));
}

export default App;
