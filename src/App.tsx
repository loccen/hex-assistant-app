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

function App() {
  const [overview, setOverview] = useState<RuntimeOverview | null>(null);
  const [health, setHealth] = useState<HealthCheckReport | null>(null);
  const [exportResult, setExportResult] = useState<DiagnosticExportResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  useEffect(() => {
    void loadOverview();
  }, []);

  const directoryReadyCount = useMemo(
    () => overview?.directories.filter((item) => item.exists).length ?? 0,
    [overview],
  );

  async function loadOverview() {
    setBusy("overview");
    setError(null);
    try {
      const data = await invoke<RuntimeOverview>("get_runtime_overview");
      setOverview(data);
    } catch (caught) {
      setError(String(caught));
    } finally {
      setBusy(null);
    }
  }

  async function runHealthCheck() {
    setBusy("health");
    setError(null);
    try {
      const data = await invoke<HealthCheckReport>("run_health_check");
      setHealth(data);
      await loadOverview();
    } catch (caught) {
      setError(String(caught));
    } finally {
      setBusy(null);
    }
  }

  async function exportDiagnostics() {
    setBusy("export");
    setError(null);
    try {
      const data = await invoke<DiagnosticExportResult>("export_diagnostic_package");
      setExportResult(data);
      await loadOverview();
    } catch (caught) {
      setError(String(caught));
    } finally {
      setBusy(null);
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
        </div>
      </header>

      {error ? <section className="error-banner">错误：{error}</section> : null}

      <section className="summary-grid" aria-label="阶段 1 状态">
        <article className="panel">
          <h2>阶段 1 基础设施</h2>
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
          <h2>默认运行参数</h2>
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
          <p className="empty-state">点击“健康检查”生成报告。阶段 1 不访问游戏接口或 ApexLOL 网络。</p>
        )}
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
          {exportResult ? (
            <p className="export-result">
              已导出：{exportResult.zipPath}，文件数 {exportResult.includedFiles}，trace id：
              {exportResult.traceId}
            </p>
          ) : (
            <p className="empty-state">诊断包会包含配置、日志、报告、样本索引和环境信息。</p>
          )}
        </article>
      </section>
    </main>
  );
}

export default App;
