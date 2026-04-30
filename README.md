# LOL 海克斯助手

LOL 海克斯助手是一个 Tauri 2 + Rust + React + TypeScript 桌面应用。项目目标是在 LOL 海克斯乱斗的无边框模式下，基于本地只读数据、显示器级截图、本地 OCR 和 ApexLOL 来源信息，为玩家展示三张海克斯符文的辅助判断信息。

助手只做信息展示和人工决策辅助，不替玩家操作游戏。

## 当前阶段

当前仓库已完成正式项目骨架，并实现了主要功能模块的代码与基础验证：

- Tauri 2 + React + TypeScript 桌面应用主界面与 Overlay 页面。
- `mise.toml` 运行时配置、应用数据目录、结构化日志和诊断导出。
- 显示器枚举、截图采样、黑屏 / 旧帧诊断和用户校准。
- PP-OCRv4 rec ONNX OCR、词库匹配和离线回放。
- Live Client Data API、状态机和低频监听编排。
- Overlay 布局、窗口控制和点击穿透处理。
- ApexLOL 查询、解析、缓存和失败兜底。
- `docs/` 项目需求、架构、验收、发布和实施依据。

当前状态更接近“主要链路已实现，Windows 实机验收仍待补齐”，而不是“仅骨架阶段”。WSL 环境下已完成基础构建和 Rust 测试；真实截图、Overlay 点击穿透、模型 / 动态库随包加载和局内端到端流程仍需在 Windows 桌面环境单独验收。

## 运行方式

本项目使用 `mise` 固定项目运行时。执行安装、构建、检查或开发命令前，先确认 `mise.toml` 已生效。

```bash
mise trust
mise install
mise exec -- npm install
mise exec -- npm run build
mise exec -- cargo check --manifest-path src-tauri/Cargo.toml
```

开发模式：

```bash
mise exec -- npm run tauri dev
```

说明：真实截图、Overlay 点击穿透和 LOL 局内流程必须在 Windows 桌面环境中验收，不能只依赖 WSL。

## 安全边界

默认运行路径只允许使用：

- LOL Live Client Data API 等本地允许的只读接口。
- 用户选择的显示器级截图。
- 用户校准区域。
- 本地离线 OCR。
- 本地缓存。
- 透明、置顶、点击穿透 Overlay。

明确禁止：

- 不注入游戏进程。
- 不 Hook 游戏、客户端或系统图形链路。
- 不读取或修改游戏内存。
- 不自动点击、不自动选择、不自动重随、不自动确认。
- 不模拟键鼠输入。
- 不默认枚举进程或窗口标题。
- 不使用样本特化 OCR 错字表。
- 不伪造 ApexLOL 数据。

## 文档依据

`docs/` 是当前项目的需求和实施依据，来自旧 POC 的立项整理文档，并在本仓库作为正式项目基线维护：

- `docs/00-index.md`
- `docs/01-product-requirements.md`
- `docs/02-architecture-and-tech-selection.md`
- `docs/03-acceptance-and-risks.md`
- `docs/04-implementation-notes.md`

旧 POC 仓库 `/home/code/hex-assistant` 只作为技术验证和证据来源，不作为本项目代码骨架。

## 阶段路线

1. 项目骨架、运行时配置和文档基线。
2. 配置、日志目录和诊断留痕基础设施。
3. 显示器级截图、黑屏 / 旧帧诊断和用户校准。
4. PP-OCRv4 rec ONNX OCR 引擎和离线回放。
5. Live Client Data API 和显式状态机。
6. 透明、置顶、点击穿透 Overlay。
7. ApexLOL 查询、本地缓存和失败兜底。
8. OCR -> ApexLOL -> Overlay 手动闭环。
9. 低频自动监听、重随刷新和完整局内验收。

每个阶段开始前需说明目标、输入、输出和验收方式；阶段完成后运行对应验证命令，并使用中文提交。
