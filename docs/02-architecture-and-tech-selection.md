# 本项目架构与技术选型

## 1. 文档定位

本文面向 LOL 海克斯助手本项目实施，基于旧 POC 已验证的技术路线整理正式项目的总体架构、模块边界、数据流、状态机、配置、日志、样本回放、OCR、Overlay、ApexLOL 查询缓存和打包方案。

本项目目标不是继续堆叠 POC，而是把已验证链路整理成可维护、可验收、可打包的产品结构。默认可用场景以 Windows 桌面环境、LOL 无边框模式为主；独占全屏只作为风险研究项，不作为 MVP 默认可用前提。

核心边界保持不变：

- 不注入游戏进程。
- 不 Hook。
- 不读取游戏内存。
- 不扫描游戏进程作为默认运行路径。
- 不枚举窗口标题作为默认运行路径。
- 不模拟点击、不自动选择、不替玩家操作。
- 只使用 Live Client Data API、显示器级截图、用户校准区域、OCR、透明置顶点击穿透 Overlay 和本地缓存。

## 2. 技术选型结论

本项目采用：

- 桌面框架：Tauri 2。
- 后端核心：Rust。
- 前端界面：React + TypeScript。
- 截图：`xcap` 显示器级截图，按用户选择显示器和校准区域裁剪。
- OCR：PP-OCRv4 rec ONNX via Rust `ort` crate，只使用 rec 模型，跳过 det / cls。
- 游戏状态：Riot / LOL Live Client Data API。
- Overlay：Tauri 透明置顶窗口，Windows 下启用点击穿透。
- 数据来源：ApexLOL 实时查询，本地缓存。
- 本地数据：用户配置、校准配置、日志、诊断样本、OCR 回放样本和 ApexLOL 缓存保存在应用数据目录。
- 打包：Tauri Windows 桌面应用，随包携带 ONNX 模型和 ONNX Runtime 动态库。

### 2.1 为什么选择 Tauri + Rust + React

Tauri 适合该项目的原因：

- 需要桌面能力：截图、透明窗口、置顶窗口、点击穿透、本地文件、打包安装。
- Rust 适合承担低层能力：截图、图像处理、OCR 推理、HTTP 请求、缓存、日志和状态机。
- React 适合实现校准向导、诊断页面、Overlay 卡片、状态可视化和调试工具。
- Tauri 的前后端边界清晰：前端负责交互和展示，Rust 负责系统能力和可信执行。

本项目不建议把核心逻辑放在浏览器前端里。截图、OCR、缓存写入、日志、ApexLOL 请求和状态机应集中在 Rust 层，前端只通过 Tauri command 调用，避免调试工具和产品逻辑分散。

### 2.2 为什么不用 Tesseract 作为正式 OCR 引擎

POC 中 Tesseract 只适合作为调试基线，不适合作为正式引擎：

- 对 LOL 海克斯中文名称的稳定性不足，容易受字体、描边、背景、缩放和裁剪影响。
- 需要依赖较重的语言包，调参与部署不够可控。
- 如果靠错字表、样本特化混淆字表或低置信度强行匹配来提升结果，会让验收结果失真。
- 当前 PP-OCRv4 rec ONNX 路线已经在 POC 的离线回归中验证 5 张真实截图、15 个 slot exact match，不需要针对样本写特化纠错。

因此本项目只把 Tesseract 保留为可选诊断基线，不进入默认识别链路，也不作为验收通过依据。

### 2.3 为什么不用 PaddlePaddle Python runtime

不采用 PaddlePaddle Python runtime 的原因：

- 桌面打包体积和依赖复杂度明显高于 Rust + ONNX Runtime 路线。
- Python sidecar 会带来运行时管理、进程通信、杀进程、环境路径、杀毒误报和升级维护问题。
- POC 已验证 PP-OCRv4 rec 模型可通过 Rust `ort` 直接推理，约 25MB 级别即可离线运行，无需 Python 运行时。
- 本项目需要稳定的 Windows 桌面分发体验，模型和 ONNX Runtime 随 Tauri 应用一起打包更可控。

开发阶段可以继续用 Python 包提供模型和 `libonnxruntime` 来源，但正式产品不依赖 Python 运行时。

### 2.4 为什么不做注入 / Hook

不做注入、Hook 或内存读取是产品安全边界，不只是技术偏好：

- 降低对游戏反作弊组件的干扰和误判风险。
- 避免读取、修改或影响游戏进程内部状态。
- 保持助手定位为信息展示和人工决策辅助，而不是自动化外挂。
- 便于把问题拆成可解释的输入：本地允许接口、截图样本、OCR 结果、来源页面和缓存。

本项目默认运行路径不得引入 DLL 注入、进程 Hook、驱动读取、内存扫描或模拟输入。诊断功能如确需读取系统环境，也必须由用户显式触发，且不能参与核心识别和展示链路。

## 3. 总体架构

本项目建议按“状态感知、截图校准、OCR 识别、数据查询、Overlay 展示、诊断回放”分层。

```text
React 主窗口
  - 校准向导
  - 诊断与回放
  - 设置与缓存管理
  - 状态可视化
        |
        | Tauri commands / events
        v
Rust 应用核心
  - Runtime Orchestrator
  - Live Client API Client
  - Capture Service
  - Calibration Service
  - OCR Service
  - ApexLOL Service + Cache
  - Overlay Controller
  - Logger / Sample Recorder
        |
        +--> xcap 显示器级截图
        +--> PP-OCRv4 rec ONNX / ort
        +--> https://127.0.0.1:2999/liveclientdata/activeplayer
        +--> https://apexlol.info
        +--> app data 本地文件
```

正式产品应避免让页面组件直接拼业务流程。前端页面发出用户动作，Rust 层的 `Runtime Orchestrator` 根据状态机决定是否截图、是否 OCR、是否查询 ApexLOL、是否刷新 Overlay。

## 4. 进程与模块划分

本项目保持单 Tauri 应用进程，不引入常驻 Python sidecar。

### 4.1 主进程

Tauri/Rust 主进程负责：

- 初始化应用数据目录。
- 加载配置和校准信息。
- 管理状态机。
- 轮询 Live Client API。
- 调度截图、OCR、查询和 Overlay。
- 写入日志、样本和缓存。
- 暴露 Tauri commands 给 React。

### 4.2 前端主窗口

React 主窗口负责：

- 首次运行引导。
- 显示器选择和截图诊断。
- 校准区域框选和回显。
- OCR 回放结果查看。
- ApexLOL 查询结果和缓存状态查看。
- Overlay 测试开关。
- 异常提示和手动修正入口。

### 4.3 Overlay 窗口

Overlay 使用独立 Tauri WebviewWindow：

- 透明背景。
- 无边框。
- 置顶。
- 不抢焦点。
- 不显示在任务栏。
- Windows 下启用点击穿透。
- 按三张海克斯卡片底部锚点分别渲染说明卡片。

Overlay 只负责显示，不负责 OCR、查询或状态判断。它接收 Rust 层推送的 slot 展示数据和可见性指令。

### 4.4 后台任务

后台任务由 Rust 管理，建议拆成：

- `live_client_poller`：低频检查 Live Client API。
- `capture_probe_worker`：在状态机允许时低频截图探测。
- `ocr_worker`：仅对三个校准名称区域做 OCR。
- `apex_lookup_worker`：按英雄 + 海克斯查询和缓存。
- `sample_recorder`：保存诊断、裁剪、增强图、JSON 和日志。

任务之间通过明确事件通信，不共享可变页面状态。

## 5. 核心数据流

### 5.1 首次校准数据流

```text
用户选择显示器
  -> xcap 截取显示器画面
  -> 保存校准截图
  -> 前端框选 3 个名称区域、3 个底部锚点、1 个展示/隐藏按钮区域
  -> 保存截图尺寸 + 归一化坐标 + Overlay 配置
  -> 对名称区域执行 OCR 测试
  -> 用户确认配置生效
```

校准配置必须保存原始截图尺寸和归一化坐标，不能只保存绝对坐标。分辨率、UI 缩放、显示模式或显示器变更后，应提示重新校准。

### 5.2 局内识别数据流

```text
Live Client API 可用
  -> 读取当前英雄和等级
  -> 等级达到海克斯档位
  -> 状态机生成待处理档位
  -> xcap 截图并检测黑屏 / 旧帧
  -> 按校准区域判断卡片和按钮状态
  -> 卡片展开时裁剪 3 个名称区域
  -> OCR 识别海克斯名称
  -> 词库保守匹配
  -> 按 英雄 + 海克斯 查询本地缓存
  -> 缓存未命中则查询 ApexLOL
  -> 写入缓存
  -> Overlay 更新三张说明卡片
```

### 5.3 重随数据流

```text
当前轮已识别 slot1/2/3
  -> 再次截图和 OCR
  -> 只比较每个 slot 的标准海克斯名称
  -> 发现某个 slot 变化
  -> 仅刷新该 slot 的 ApexLOL 查询和 Overlay 内容
  -> 未变化 slot 保留旧内容
```

低置信度结果不得覆盖高置信度旧结果。某个 slot 连续失败时，应显示异常提示并保留调试样本。

## 6. 状态机设计

建议本项目使用显式状态机，不用散落的布尔值拼流程。

### 6.1 状态定义

```text
Idle
  普通监听。Live Client API 不可用或未进入对局。

InGameWatching
  Live Client API 可用，已读取英雄和等级，但未达到待处理档位。

AugmentLevelReady
  等级达到 3 / 7 / 11 / 15，存在未完成档位，开始低频截图探测。

PanelCollapsed
  展示 / 隐藏按钮存在，但三张卡片不可见。Overlay 隐藏，不标记完成。

PanelExpanded
  三张卡片可见。允许 OCR 和 Overlay 展示。

ResolvingSlots
  正在识别三个 slot，并查询 ApexLOL。

ShowingOverlay
  三张说明卡片已展示，继续低频监测重随和收起。

RoundCompleteCandidate
  卡片不可见，按钮也消失，作为当前轮完成候选，需要短时间确认。

RoundCompleted
  当前档位完成，写入已处理档位，检查是否还有待处理档位。

Suspended
  截图黑屏、旧帧、校准缺失、OCR 连续失败或 Overlay 异常，暂停自动展示并提示用户处理。
```

### 6.2 关键规则

- 低于 3 级时不截图、不 OCR、不显示 Overlay。
- 等级达到 3 / 7 / 11 / 15 后，根据已完成档位生成待处理队列。
- 卡片可见优先于按钮区域；鼠标悬停导致详情浮层遮挡按钮时，不应隐藏 Overlay。
- 按钮存在但卡片不可见表示面板收起，不表示选择完成。
- 卡片不可见且按钮消失只能作为完成候选，必须短时间复核，避免瞬时截图误判。
- 多档位待处理时按队列逐轮处理，不能只保存一个当前档位。
- 重随只刷新变化 slot，不能重置整个阶段。
- 截图黑屏或疑似旧帧时，不进入 OCR。
- OCR 低置信度时，不直接查询 ApexLOL。

## 7. 配置文件设计

建议所有本地配置放在 Tauri 应用数据目录下，按用途拆分。

```text
app-data/
  config/
    settings.json
    runtime-state.json
  calibration/
    profile.json
    snapshots/
  ocr-debug/
  diagnostics/
  replay/
  apex-cache/
    cache.json
  logs/
    app.log
    overlay-debug.log
    test-events.jsonl
```

### 7.1 `settings.json`

保存用户偏好和运行参数：

```json
{
  "version": 1,
  "language": "zh-CN",
  "capture": {
    "preferredMonitorId": "monitor-0",
    "pollIntervalMs": 1000,
    "retryDelayMs": 200,
    "defaultDisplayMode": "borderless"
  },
  "ocr": {
    "engine": "ppocr-v4-rec-onnx",
    "minConfidence": 0.85,
    "minMatchScore": 0.9
  },
  "overlay": {
    "enabled": true,
    "clickThrough": true,
    "gap": 8,
    "maxHeight": 120
  },
  "apexLol": {
    "cacheTtlHours": 168,
    "requestTimeoutMs": 6000
  }
}
```

### 7.2 `calibration/profile.json`

保存校准配置：

```json
{
  "version": 1,
  "profileName": "default",
  "monitorId": "monitor-0",
  "monitorName": "Primary Monitor",
  "screenshotWidth": 2560,
  "screenshotHeight": 1440,
  "dpiScale": 1.0,
  "displayModeNote": "无边框",
  "language": "zh-CN",
  "nameRegions": [
    { "slot": 1, "xRatio": 0.2188, "yRatio": 0.3882, "widthRatio": 0.1777, "heightRatio": 0.0243 },
    { "slot": 2, "xRatio": 0.4043, "yRatio": 0.3882, "widthRatio": 0.1797, "heightRatio": 0.0243 },
    { "slot": 3, "xRatio": 0.5898, "yRatio": 0.3882, "widthRatio": 0.1797, "heightRatio": 0.0243 }
  ],
  "bottomAnchors": [
    { "slot": 1, "xRatio": 0.22, "yRatio": 0.72, "widthRatio": 0.18, "heightRatio": 0.04 },
    { "slot": 2, "xRatio": 0.41, "yRatio": 0.72, "widthRatio": 0.18, "heightRatio": 0.04 },
    { "slot": 3, "xRatio": 0.59, "yRatio": 0.72, "widthRatio": 0.18, "heightRatio": 0.04 }
  ],
  "toggleButtonRegion": {
    "xRatio": 0.45,
    "yRatio": 0.88,
    "widthRatio": 0.10,
    "heightRatio": 0.05
  },
  "overlay": {
    "gap": 8,
    "maxHeight": 120
  }
}
```

### 7.3 `apex-cache/cache.json`

缓存粒度为英雄 + 海克斯：

```json
{
  "version": 1,
  "entries": {
    "VI::吞噬灵魂": {
      "championName": "Vi",
      "augmentName": "吞噬灵魂",
      "rating": "顶级",
      "summary": "与当前英雄机制适配。",
      "tip": "根据来源页面解析的提醒。",
      "source": "ApexLOL",
      "sourceUrl": "https://apexlol.info/...",
      "fetchedAt": "2026-04-29T00:00:00Z",
      "cacheHit": false,
      "status": "ok",
      "error": null
    }
  }
}
```

请求失败、页面结构不匹配或解析不到可靠内容时，必须返回“暂无数据”，不能伪造说明。

## 8. 日志与样本回放

本项目必须把“能复盘”作为核心能力，而不是临时调试工具。

### 8.1 日志分类

- 应用日志：启动、配置加载、状态机迁移、异常。
- 截图诊断日志：显示器、显示模式、地图备注、截图策略、耗时、黑屏判断、旧帧判断、样本路径。
- OCR 日志：触发原因、slot、裁剪坐标、候选图、原始文本、置信度、词库匹配、耗时。
- Overlay 日志：窗口创建、显示器、透明状态、点击穿透状态、卡片坐标、隐藏原因。
- ApexLOL 日志：请求 URL、缓存命中、解析状态、失败原因。
- 测试事件日志：人工验收动作和结果。

### 8.2 样本保存

截图诊断每次至少保存：

- `diagnostic.log`
- `diagnostic.json`
- 原始截图 PNG
- 黑屏和旧帧判断结果

OCR 每次识别建议保存：

- `raw.png`
- `focused.png`
- `tight-line.png`
- `enhanced.png`
- `ocr-report.json`

### 8.3 样本回放

本项目应提供 CLI 或内置回放入口：

```text
replay-ocr --profile calibration/profile.json --image sample.png --out replay/run-id
```

回放必须不依赖实时游戏环境，输入校准配置和截图样本即可复现裁剪、增强、OCR、匹配和最终选择。离线 OCR 环境不可用时，也要输出裁剪坐标和候选图片，便于人工判断是定位问题还是识别问题。

## 9. OCR 引擎设计

### 9.1 正式引擎

正式 OCR 引擎使用 PP-OCRv4 rec ONNX via Rust `ort` crate：

- 输入为已校准的名称区域裁剪图。
- 图片预处理为高度 48，宽度等比缩放，RGB 归一化到 `[-1, 1]`。
- 推理输入形状为 `[1, 3, 48, W]`。
- 字符表从模型 metadata 的 `character` 字段读取。
- 使用 CTC 贪心解码。
- 输出标准结构：文本、平均置信度、耗时。

不使用 det 的原因是名称区域已由用户校准，不需要全图检测文字位置。

不使用 cls 的原因是海克斯名称方向固定水平，不需要方向分类。

### 9.2 词库匹配

OCR 原始文本不得直接用于 ApexLOL 查询。正式链路应加入海克斯名称词库匹配：

```text
OCR 原始文本
  -> 规范化
  -> 候选词库相似度计算
  -> 置信度阈值 + 相似度阈值
  -> 标准海克斯名称
```

低置信度或低相似度时：

- 显示疑似结果。
- 允许用户手动修正。
- 保存调试图和候选输出。
- 不覆盖高置信度旧结果。
- 不直接查询 ApexLOL。

## 10. Overlay 设计

Overlay 的产品行为：

- 三张说明卡片分别对应左、中、右三个 slot。
- 位置来自 `bottomAnchors` 和 Overlay 间距配置。
- 卡片在海克斯面板展开时显示，在面板收起或选择阶段结束后隐藏。
- 不遮挡海克斯名称、重随按钮、选择按钮和底部展示 / 隐藏按钮。
- 不抢焦点，不拦截点击。

Windows 实现要点：

- Tauri WebviewWindow。
- `decorations(false)`。
- `always_on_top(true)`。
- `focused(false)` / `focusable(false)`。
- `skip_taskbar(true)`。
- 透明背景。
- `set_ignore_cursor_events(true)`。
- 对 WebView2 子窗口补充点击穿透处理。
- 记录窗口句柄、外层窗口位置、子窗口信息和点击穿透结果。

非 Windows 构建可以保留开发调试能力，但真实截图、Overlay 可见性和点击穿透验收必须在 Windows 桌面环境完成。

## 11. ApexLOL 查询与缓存

### 11.1 查询策略

查询输入：

```text
当前英雄 + 标准海克斯名称
```

查询顺序：

```text
读取本地缓存
  -> 命中则立即展示
  -> 未命中则请求 ApexLOL
  -> 优先解析中文页面
  -> 必要时尝试英文页面
  -> 解析成功后写入缓存
  -> 解析失败显示“暂无数据”
```

展示内容必须包含：

- 评级。
- 英雄 × 海克斯名称。
- 摘要。
- 提醒。
- 数据来源：ApexLOL。
- 来源页面入口。

### 11.2 缓存策略

缓存 key 使用规范化后的英雄名和海克斯名。缓存值保留来源 URL、获取时间、状态和错误信息。

本项目建议加入：

- TTL。
- 手动刷新。
- 按英雄清理。
- 按来源 URL 追踪。
- 失败结果短 TTL，避免局内反复请求。

解析失败时不缓存为永久有效结果。失败兜底只能证明应用不会崩溃，不能作为真实数据解析通过的验收结论。

## 12. 打包方案

本项目以 Windows Tauri 应用为主要交付目标。

### 12.1 打包内容

随应用打包：

- React 前端产物。
- Rust/Tauri 主程序。
- PP-OCRv4 rec ONNX 模型。
- ONNX Runtime Windows 动态库。
- 默认配置模板。
- 海克斯名称词库。

不随应用打包：

- Python runtime。
- PaddlePaddle runtime。
- Tesseract 语言包作为默认运行依赖。
- 训练数据或大规模源站数据。

### 12.2 开发与生产差异

开发环境可以通过 `ORT_DYLIB_PATH` 指向本机 `libonnxruntime.so` 或 `onnxruntime.dll`，模型可以复用开发环境中的 RapidOCR 模型文件。

生产环境必须使用应用资源目录中的模型和 ONNX Runtime 动态库，启动时自动解析资源路径，不要求用户配置环境变量。

### 12.3 构建验证

代码改动必须通过：

```bash
mise exec -- npm run build
```

涉及 Rust、Tauri、OCR 或打包改动时，还应补充：

```bash
mise exec -- npm run tauri build
```

只修改文档时至少执行：

```bash
git diff --check
```

## 13. POC 已验证证据

旧 POC 已验证的内容：

- 显示器级截图诊断已实现，Windows 下使用 `xcap`，包含目标截图、主显示器截图、中心区域截图三类策略。
- 截图诊断会记录黑屏判断、旧帧判断、亮度统计、截图耗时、样本路径和历史匹配信息。
- 校准配置已实现截图尺寸、显示器信息、三个名称区域、三个底部锚点、展示 / 隐藏按钮区域和 Overlay 参数保存。
- Overlay POC 已实现独立透明置顶窗口、按校准锚点生成三张测试卡片、Windows 点击穿透请求和日志记录。
- Live Client API POC 已限定读取 `https://127.0.0.1:2999/liveclientdata/activeplayer`，用于获取当前玩家数据。
- ApexLOL 查询缓存 POC 已实现按英雄 + 海克斯查询、缓存命中、中文 / 英文页面尝试、失败返回“暂无数据”。
- OCR 核心引擎已实现 PP-OCRv4 rec ONNX via Rust `ort`，包含模型 metadata 字符表读取、预处理、推理和 CTC 解码。
- OCR 离线回归评估二进制已实现，POC 文档和评估代码记录 5 张 2560x1440 真实截图、15 个 slot exact match，未使用符文名错字表或样本特化纠错。
- 当前截图实测记录显示，莲华栈桥、嚎哭深渊、屠夫之桥三张地图的无边框模式均出现连续截图刷新，结果为 `black=false` 且 `stale=false`。

旧 POC 的 OCR race 总报告位于 `artifacts/ocr-race/results/overall-report.md`，Rust ORT 回归报告位于 `artifacts/ocr-race/results/rapidocr-rust-regression/report.md`。这些报告证明技术路线可行，但本项目仍需重新生成自己的回归报告，不能直接把 POC 报告当作本项目验收通过。

## 14. 未验证风险

本项目必须继续跟踪这些风险：

- 独占全屏截图：当前样本显示三张地图的全屏模式都没有通过连续两次刷新标准，存在旧帧风险。
- Overlay 真实局内表现：透明、置顶、点击穿透在 Windows 桌面真实游戏窗口、无边框和全屏场景仍需完整验收。
- 多显示器和 DPI：显示器坐标、逻辑尺寸、物理尺寸、缩放比例和 WebView2 非客户区偏移仍需覆盖更多机器。
- OCR 泛化：当前离线回归样本数量有限，还需覆盖更多地图、分辨率、画质、UI 缩放、低清和遮挡样本。
- ApexLOL 解析稳定性：源站页面结构变化会影响解析，必须保留“暂无数据”和来源跳转兜底。
- 状态机长时间运行：多档位连续处理、重随 slot 级刷新、异常恢复、对局切换和长时间后台运行仍需正式验收。
- 打包资源路径：生产环境模型、ONNX Runtime 动态库和 Tauri 资源路径需要单独验证。
- 性能和功耗：截图探测、OCR 和网络请求必须保持低频，避免局内持续高负载。

## 15. 本项目建议目录结构

```text
hex-assistant/
  README.md
  docs/
    architecture/
      01-product-boundary.md
      02-architecture-and-tech-selection.md
      03-state-machine.md
      04-data-and-cache.md
      05-qa-and-acceptance.md
    operations/
      calibration-guide.md
      replay-guide.md
  apps/
    desktop/
      package.json
      src/
        main.tsx
        app/
        pages/
          CalibrationPage.tsx
          DiagnosticsPage.tsx
          SettingsPage.tsx
        overlay/
          OverlayApp.tsx
        components/
        services/
          tauriCommands.ts
      src-tauri/
        Cargo.toml
        tauri.conf.json
        build.rs
        src/
          main.rs
          lib.rs
          commands/
            capture.rs
            calibration.rs
            live_client.rs
            ocr.rs
            overlay.rs
            apex_lol.rs
            logs.rs
          core/
            orchestrator.rs
            state_machine.rs
            config.rs
            events.rs
          services/
            capture_service.rs
            ocr_service.rs
            apex_lol_service.rs
            cache_service.rs
            overlay_service.rs
            sample_recorder.rs
          model/
            calibration.rs
            augment.rs
            runtime_state.rs
          bin/
            replay_ocr.rs
            ocr_eval.rs
        resources/
          models/
            ch_PP-OCRv4_rec_infer.onnx
          onnxruntime/
            onnxruntime.dll
          dictionaries/
            augments.zh-CN.json
  crates/
    hex-core/
      src/
        state_machine.rs
        matching.rs
        types.rs
    hex-ocr/
      src/
        ppocr_rec.rs
        preprocess.rs
        ctc.rs
  scripts/
    replay-ocr.mjs
    collect-samples.mjs
  tests/
    fixtures/
      ocr/
      calibration/
```

拆分原则：

- `apps/desktop` 放 Tauri 桌面应用。
- `crates/hex-core` 放可单元测试的状态机、类型和匹配逻辑。
- `crates/hex-ocr` 放 OCR 预处理、推理和解码，便于 CLI 和桌面应用复用。
- `resources/` 放生产打包资源。
- `docs/` 放面向实施和验收的中文文档。
- `tests/fixtures` 放可公开或脱敏的样本，不混入用户本机应用数据目录。

## 16. 实施优先级

建议本项目按以下顺序实施：

1. 建立 Tauri + Rust + React 基础工程和应用数据目录。
2. 实现配置、日志和样本保存。
3. 实现显示器级截图诊断和校准配置。
4. 实现 OCR 服务和离线回放。
5. 实现 Live Client API 读取和状态机。
6. 实现 ApexLOL 查询、解析、缓存和失败兜底。
7. 实现 Overlay 窗口、定位和点击穿透。
8. 串联局内状态机和 slot 级刷新。
9. 完成 Windows 打包和资源路径验证。
10. 按验收文档补齐真实样本、无边框模式、多显示器和异常场景记录。

每一步都必须留下可复查的日志、配置或样本。未真实验收的能力只能标为“已实现 POC”或“待验收”，不能写成“已通过”。
