import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
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
  const [monitors, setMonitors] = useState<MonitorDiagnostic[]>([]);
  const [selectedMonitorId, setSelectedMonitorId] = useState("");
  const [mode, setMode] = useState<"status" | "calibration">("status");
  const [capture, setCapture] = useState<CaptureSampleReport | null>(null);
  const [screenshot, setScreenshot] = useState<ScreenshotDataUrl | null>(null);
  const [marks, setMarks] = useState<MarkState>(() => emptyMarkState());
  const [activeRegion, setActiveRegion] = useState<RegionKey>("name-0");
  const [drag, setDrag] = useState<DragState | null>(null);
  const [countdown, setCountdown] = useState<number | null>(null);
  const [ocrReport, setOcrReport] = useState<CalibratedNameOcrReport | null>(null);
  const [toast, setToast] = useState<Toast | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void boot();

    const unlistenRecalibrate = listen("hex-assistant://recalibrate", () => {
      beginCalibration();
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
    await Promise.all([loadCalibrationSilently(), refreshRuntime(), loadMonitors()]);
  }

  async function runCommand<T>(
    key: string,
    command: string,
    args?: Record<string, unknown>,
    silent = false,
  ): Promise<T | null> {
    setBusy(key);
    if (!silent) {
      setError(null);
    }
    try {
      return await invoke<T>(command, args);
    } catch (caught) {
      if (!silent) {
        setError(String(caught));
      }
      return null;
    } finally {
      setBusy(null);
    }
  }

  async function loadCalibrationSilently() {
    try {
      const loaded = await invoke<CalibrationProfileResult>("load_calibration_profile");
      setProfile(loaded);
      setCalibrated(true);
      setMode("status");
    } catch {
      setProfile(null);
      setCalibrated(false);
      setMode("calibration");
    }
  }

  async function refreshRuntime(silent = false) {
    const data = await runCommand<RuntimeLoopSnapshot>(
      "runtime-status",
      "get_runtime_orchestrator_status",
      undefined,
      silent,
    );
    if (data) {
      setRuntime(data);
    }
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

  function beginCalibration() {
    setMode("calibration");
    setCapture(null);
    setScreenshot(null);
    setMarks(emptyMarkState());
    setActiveRegion("name-0");
    setOcrReport(null);
    setError(null);
    setToast({ tone: "info", message: "请按向导重新完成校准。" });
    void loadMonitors();
  }

  async function startListener() {
    const request: RuntimeTriggerRequest = {
      panelSnapshot: { panelState: "collapsed", choices: [], selectedSlot: null },
    };
    const data = await runCommand<RuntimeLoopSnapshot>("runtime-start", "start_runtime_listener", {
      request,
    });
    if (data) {
      setRuntime(data);
      setToast({ tone: "success", message: "助手已开始监听。" });
    }
  }

  async function stopListener() {
    const data = await runCommand<RuntimeLoopSnapshot>("runtime-stop", "stop_runtime_listener");
    if (data) {
      setRuntime(data);
      setToast({ tone: "info", message: "助手已停止监听。" });
    }
  }

  async function captureAfterDelay() {
    const preferredMonitorId = selectedMonitorId === "" ? null : Number.parseInt(selectedMonitorId, 10);
    setCountdown(5);
    setToast({ tone: "info", message: "请切回游戏画面，助手将在 5 秒后截图。" });
    for (let left = 4; left >= 0; left -= 1) {
      await delay(1000);
      setCountdown(left === 0 ? null : left);
    }

    const data = await runCommand<CaptureSampleReport>("capture", "capture_monitor_sample", {
      preferredMonitorId,
    });
    if (!data) {
      return;
    }
    await loadCaptureIntoCalibration(data, "截图完成，可以关闭或离开自定义游戏。");
  }

  async function loadLatestCapture() {
    const data = await runCommand<CaptureSampleReport>("latest-capture", "load_latest_capture_sample");
    if (!data) {
      return;
    }
    await loadCaptureIntoCalibration(data, "已加载最近截图样本，可以直接继续校准。");
  }

  async function loadCaptureIntoCalibration(data: CaptureSampleReport, successMessage: string) {
    setCapture(data);
    setOcrReport(null);
    setMarks(emptyMarkState());
    setActiveRegion("name-0");
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
        message: "校准已保存。后续助手将缩到托盘无感运行，可通过托盘重新校准。",
      });
    }
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
          setMarks={setMarks}
          completedMarks={completedMarks}
          drag={drag}
          setDrag={setDrag}
          countdown={countdown}
          busy={busy}
          readyToCheck={readyToCheck}
          readyToSave={readyToSave}
          ocrReport={ocrReport}
          onRefreshMonitors={loadMonitors}
          onCapture={captureAfterDelay}
          onLoadLatestCapture={loadLatestCapture}
          onRunOcrCheck={runOcrCheck}
          onSave={saveCalibration}
        />
      );
    }
    return (
      <StatusPanel
        profile={profile}
        runtime={runtime}
        busy={busy}
        onStart={startListener}
        onStop={stopListener}
        onRefresh={() => void refreshRuntime()}
        onRecalibrate={beginCalibration}
      />
    );
  }

  return (
    <main className="app-shell">
      <header className="topbar">
        <div>
          <p className="eyebrow">LOL 海克斯助手</p>
          <h1>{mode === "calibration" || calibrated === false ? "首次校准向导" : "助手状态"}</h1>
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
  busy,
  onStart,
  onStop,
  onRefresh,
  onRecalibrate,
}: {
  profile: CalibrationProfileResult | null;
  runtime: RuntimeLoopSnapshot | null;
  busy: string | null;
  onStart: () => void;
  onStop: () => void;
  onRefresh: () => void;
  onRecalibrate: () => void;
}) {
  return (
    <section className="panel status-panel">
      <dl className="status-grid">
        <div>
          <dt>运行状态</dt>
          <dd>{runtime?.listening ? "监听中" : "未监听"}</dd>
        </div>
        <div>
          <dt>校准状态</dt>
          <dd>{profile ? "已完成" : "未完成"}</dd>
        </div>
        <div>
          <dt>当前阶段</dt>
          <dd>{runtime ? runtimeStatusText[runtime.state.status] ?? runtime.state.status : "待启动"}</dd>
        </div>
        <div>
          <dt>待处理档位</dt>
          <dd>{runtime && runtime.state.pendingTiers.length > 0 ? runtime.state.pendingTiers.join(" / ") : "无"}</dd>
        </div>
        <div>
          <dt>最近状态</dt>
          <dd>{runtime?.lastErrorCode ?? runtime?.state.pauseReason ?? "正常"}</dd>
        </div>
      </dl>
      <div className="button-row">
        <button type="button" onClick={onStart} disabled={busy !== null || runtime?.listening === true || !profile}>
          开始监听
        </button>
        <button type="button" onClick={onStop} disabled={busy !== null || runtime?.listening !== true}>
          停止监听
        </button>
        <button type="button" onClick={onRefresh} disabled={busy !== null}>
          刷新状态
        </button>
        <button type="button" onClick={onRecalibrate} disabled={busy !== null}>
          重新校准
        </button>
      </div>
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
  setMarks,
  completedMarks,
  drag,
  setDrag,
  countdown,
  busy,
  readyToCheck,
  readyToSave,
  ocrReport,
  onRefreshMonitors,
  onCapture,
  onLoadLatestCapture,
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
  setMarks: (setter: (current: MarkState) => MarkState) => void;
  completedMarks: number;
  drag: DragState | null;
  setDrag: (drag: DragState | null) => void;
  countdown: number | null;
  busy: string | null;
  readyToCheck: boolean;
  readyToSave: boolean;
  ocrReport: CalibratedNameOcrReport | null;
  onRefreshMonitors: () => void;
  onCapture: () => void;
  onLoadLatestCapture: () => void;
  onRunOcrCheck: () => void;
  onSave: () => void;
}) {
  return (
    <section className="wizard-grid">
      <article className="panel guide-panel">
        <h2>准备游戏画面</h2>
        <ol className="step-list">
          <li>打开英雄联盟。</li>
          <li>创建一局自定义游戏。</li>
          <li>选择海克斯乱斗并进入游戏。</li>
          <li>在游戏设置中选择无边框。</li>
          <li>回到助手，选择目标显示器。</li>
          <li>点击“5 秒后截图”，立刻切回游戏并等待。</li>
          <li>如果之前已经截过图，也可以直接点击“加载最近截图”。</li>
          <li>截图完成后可以关闭或离开游戏。</li>
        </ol>
        <div className="capture-controls">
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
            <button type="button" onClick={onLoadLatestCapture} disabled={busy !== null}>
              加载最近截图
            </button>
            <button type="button" onClick={onCapture} disabled={busy !== null || countdown !== null}>
              {countdown ? `${countdown} 秒后截图` : "5 秒后截图"}
            </button>
          </div>
        </div>
      </article>

      <article className="panel marking-panel">
        <div className="marking-header">
          <div>
            <h2>标记校准区域</h2>
            <p>
              已完成 {completedMarks}/{regionDefinitions.length}，当前：{regionDefinitions.find((item) => item.key === activeRegion)?.label}
            </p>
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
          />
          <RegionList
            marks={marks}
            activeRegion={activeRegion}
            setActiveRegion={setActiveRegion}
            onClear={() => {
              setMarks(() => emptyMarkState());
              setActiveRegion("name-0");
            }}
          />
        </div>
      </article>

      <article className="panel check-panel">
        <div className="panel-heading">
          <h2>三槽识别校验</h2>
          <button type="button" onClick={onRunOcrCheck} disabled={busy !== null || !readyToCheck}>
            生成校验结果
          </button>
        </div>
        {ocrReport ? (
          <div className="ocr-grid">
            {ocrReport.slots.map((slot) => (
              <div key={slot.slot} className={slot.finalName ? "ocr-card pass" : "ocr-card warn"}>
                <strong>{slotText[slot.slot]}</strong>
                <span>{slot.finalName ?? "未确认"}</span>
                <small>
                  原文：{slot.rawText || "-"}；置信度 {slot.confidence.toFixed(2)}；匹配{" "}
                  {slot.matchScore.toFixed(2)}
                  {slot.failureReason ? `；${slot.failureReason}` : ""}
                </small>
              </div>
            ))}
          </div>
        ) : (
          <p className="muted">完成 7 个标记后生成校验结果，确认三槽名称区域和底部定位正确。</p>
        )}
        <div className="button-row final-row">
          <button type="button" onClick={onSave} disabled={busy !== null || !readyToSave}>
            确认并保存配置
          </button>
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
}: {
  screenshot: ScreenshotDataUrl | null;
  capture: CaptureSampleReport | null;
  marks: MarkState;
  activeRegion: RegionKey;
  drag: DragState | null;
  setDrag: (drag: DragState | null) => void;
  setMarks: (setter: (current: MarkState) => MarkState) => void;
}) {
  const ref = useRef<HTMLDivElement | null>(null);
  const width = capture?.image.width ?? 16;
  const height = capture?.image.height ?? 9;
  const activeDefinition = regionDefinitions.find((definition) => definition.key === activeRegion)!;
  const [zoom, setZoom] = useState(1);

  useEffect(() => {
    setZoom(1);
  }, [screenshot?.path]);

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
      <div className="preview-meta">
        <span>{capture ? `截图尺寸：${capture.image.width} x ${capture.image.height}` : "等待截图"}</span>
        <span>{capture ? `截图时间：${capture.capturedAt}` : "按向导完成自定义游戏画面后截图"}</span>
      </div>
      <div className="preview-toolbar">
        <span>预览缩放 {Math.round(zoom * 100)}%</span>
        <div className="button-row">
          <button type="button" onClick={() => setZoom((current) => clamp(Number((current - 0.25).toFixed(2)), 0.5, 3))}>
            缩小
          </button>
          <button type="button" onClick={() => setZoom(1)}>
            还原
          </button>
          <button type="button" onClick={() => setZoom((current) => clamp(Number((current + 0.25).toFixed(2)), 0.5, 3))}>
            放大
          </button>
        </div>
      </div>
      <div className="preview-viewport">
        <div
          ref={ref}
          className="screenshot-preview"
          style={{
            width: `${Math.max(1, Math.round(width * zoom))}px`,
            height: `${Math.max(1, Math.round(height * zoom))}px`,
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
              <strong>等待截图</strong>
              <span>截图后会在这里显示游戏画面。</span>
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

function RegionList({
  marks,
  activeRegion,
  setActiveRegion,
  onClear,
}: {
  marks: MarkState;
  activeRegion: RegionKey;
  setActiveRegion: (key: RegionKey) => void;
  onClear: () => void;
}) {
  return (
    <aside className="region-panel">
      <div className="region-actions">
        <button type="button" onClick={onClear}>
          清空标记
        </button>
      </div>
      <div className="region-list">
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
                <small>{definition.help}</small>
              </span>
              <em>{value ? formatRegionValue(value) : "未标记"}</em>
            </button>
          );
        })}
      </div>
    </aside>
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
