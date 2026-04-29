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

type DiagnosticExportResult = {
  traceId: string;
  zipPath: string;
  includedFiles: number;
};

type CaptureSampleReport = {
  capturedAt: string;
  monitor: {
    id: number;
    name: string;
    friendlyName: string;
    width: number;
    height: number;
    primary: boolean;
  };
  image: {
    width: number;
    height: number;
    meanLuma: number;
    blackScreen: boolean;
    frameHash: string;
  };
  pngPath: string;
  jsonPath: string;
  previousFrameHash?: string | null;
  staleFrame: boolean;
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
    pendingTier?: number | null;
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

type ApexCacheReport = {
  cachePath: string;
  generatedAt: string;
  totalEntries: number;
  okEntries: number;
  failedEntries: number;
  expiredEntries: number;
  entries: Array<{
    cacheKey: string;
    championName: string;
    augmentName: string;
    status: string;
    expired: boolean;
    error?: string | null;
  }>;
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

function App() {
  const isOverlayView = new URLSearchParams(window.location.search).get("view") === "overlay";
  return isOverlayView ? <OverlayPage /> : <DiagnosticApp />;
}

function DiagnosticApp() {
  const [overview, setOverview] = useState<RuntimeOverview | null>(null);
  const [health, setHealth] = useState<HealthCheckReport | null>(null);
  const [diagnosticExport, setDiagnosticExport] = useState<DiagnosticExportResult | null>(null);
  const [releaseExport, setReleaseExport] = useState<DiagnosticExportResult | null>(null);
  const [captureReport, setCaptureReport] = useState<CaptureSampleReport | null>(null);
  const [ocrStatus, setOcrStatus] = useState<OcrResourceStatus | null>(null);
  const [ocrReplay, setOcrReplay] = useState<OfflineReplayReport | null>(null);
  const [liveClient, setLiveClient] = useState<ActivePlayerSnapshot | null>(null);
  const [stateResult, setStateResult] = useState<StateMachineResult | null>(null);
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
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  useEffect(() => {
    void loadOverview();
  }, []);

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
      setError(String(caught));
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
    const data = await runCommand<CaptureSampleReport>("capture", "capture_monitor_sample", {
      preferredMonitorId: null,
    });
    if (data) {
      setCaptureReport(data);
    }
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

  async function loadApexCacheReport() {
    const data = await runCommand<ApexCacheReport>("apex-cache", "build_apex_cache_report");
    if (data) {
      setApexReport(data);
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

      {error ? <section className="error-banner">错误：{error}</section> : null}

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
            <button type="button" onClick={captureSample} disabled={busy !== null}>
              采集样本
            </button>
          </div>
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
                  {captureReport.image.blackScreen ? "黑屏" : "可见"} ·{" "}
                  {captureReport.staleFrame ? "重复帧" : "新帧"}
                </dd>
              </div>
              <div>
                <dt>样本文件</dt>
                <dd>{captureReport.pngPath}</dd>
              </div>
            </dl>
          ) : (
            <p className="empty-state">暂无截图样本。</p>
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
            <h2>Apex 缓存</h2>
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
              <p className="trace-line">{apexReport.cachePath}</p>
            </>
          ) : (
            <p className="empty-state">暂无 Apex 缓存报告。</p>
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
                <dt>窗口区域</dt>
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
