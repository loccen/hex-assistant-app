import "./App.css";

const phases = [
  "项目骨架与运行时配置",
  "配置、日志和诊断留痕",
  "显示器截图与用户校准",
  "PP-OCRv4 rec ONNX 离线识别",
  "Live Client Data API 与状态机",
  "透明置顶点击穿透 Overlay",
  "ApexLOL 查询与本地缓存",
  "局内低频监听与完整验收",
];

const boundaries = [
  "不注入",
  "不 Hook",
  "不读内存",
  "不自动点击",
  "不自动选择",
  "不模拟键鼠",
];

function App() {
  return (
    <main className="app-shell">
      <section className="hero">
        <p className="eyebrow">LOL Hex Assistant</p>
        <h1>LOL 海克斯助手</h1>
        <p className="summary">
          面向无边框模式的局内信息提示工具。后续阶段会接入显示器级截图、本地 OCR、Live Client Data API、ApexLOL 查询和透明 Overlay。
        </p>
      </section>

      <section className="status-grid" aria-label="项目状态">
        <article className="panel">
          <h2>当前阶段</h2>
          <p className="stage-name">第一阶段：正式项目骨架</p>
          <p>
            当前仓库只保留干净的 Tauri、Rust、React 和 TypeScript 基础结构，旧 POC 仅作为需求和技术验证依据。
          </p>
        </article>

        <article className="panel">
          <h2>安全边界</h2>
          <div className="boundary-list">
            {boundaries.map((item) => (
              <span key={item}>{item}</span>
            ))}
          </div>
        </article>
      </section>

      <section className="roadmap" aria-label="阶段路线">
        <h2>阶段路线</h2>
        <ol>
          {phases.map((phase, index) => (
            <li key={phase} className={index === 0 ? "active" : ""}>
              <span>{String(index + 1).padStart(2, "0")}</span>
              {phase}
            </li>
          ))}
        </ol>
      </section>
    </main>
  );
}

export default App;
