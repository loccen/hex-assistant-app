import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

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
  bounds: {
    x: number;
    y: number;
    width: number;
    height: number;
  };
  clickThrough: {
    requested: boolean;
    platform: string;
    status: string;
    message: string;
  };
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
  const [overview, setOverview] = useState<RuntimeOverview | null>(null);
  const [health, setHealth] = useState<HealthCheckReport | null>(null);
  const [diagnosticExport, setDiagnosticExport] = useState<DiagnosticExportResult | null>(null);
  const [releaseExport, setReleaseExport] = useState<DiagnosticExportResult | null>(null);
  const [captureReport, setCaptureReport] = useState<CaptureSampleReport | null>(null);
  const [ocrStatus, setOcrStatus] = useState<OcrResourceStatus | null>(null);
  const [ocrReplay, setOcrReplay] = useState<OfflineReplayReport | null>(null);
  const [liveClient, setLiveClient] = useState<ActivePlayerSnapshot | null>(null);
  const [stateResult, setStateResult] = useState<StateMachineResult | null>(null);
  const [apexResult, setApexResult] = useState<ApexLookupResult | null>(null);
  const [apexReport, setApexReport] = useState<ApexCacheReport | null>(null);
  const [overlayReport, setOverlayReport] = useState<OverlayOperationReport | null>(null);
  const [ocrTexts, setOcrTexts] = useState({
    leftText: "棱彩门票",
    centerText: "好事成双",
    rightText: "利滚利",
  });
  const [simulator, setSimulator] = useState({
    championName: "Ahri",
    level: 7,
    panelExpanded: true,
    selectedSlot: "",
  });
  const [apexQuery, setApexQuery] = useState({
    championName: "放逐之刃",
    augmentName: "灵魂虹吸",
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
        anchor: "topRight",
        width: 360,
        height: 96,
        gap: overview?.settings.overlay.gap ?? 8,
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
                <dt>点击穿透</dt>
                <dd>
                  {overlayReport.clickThrough.status} · {overlayReport.clickThrough.message}
                </dd>
              </div>
            </dl>
          ) : (
            <p className="empty-state">暂无 Overlay 报告。</p>
          )}
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

export default App;
