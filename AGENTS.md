# LOL 海克斯助手协作说明

## 项目边界

- 本仓库 `/home/code/hex-assistant-app` 是正式项目，旧 POC `/home/code/hex-assistant` 只能作为技术验证和文档依据，不在旧 POC 中继续叠加功能。
- 技术栈固定为 Tauri 2 + Rust + React + TypeScript。
- 运行时由 `mise.toml` 管理。执行安装、构建、检查、开发命令前先检查项目运行时配置，所有命令通过 `mise exec -- ...` 执行。
- 文档、代码注释和提交信息默认使用中文。

## 安全边界

默认运行链路只允许使用本地允许的只读接口、显示器级截图、用户校准区域、本地 OCR、本地缓存和透明置顶点击穿透 Overlay。

严禁引入或默认使用：

- 注入游戏进程。
- Hook 游戏、客户端或系统图形链路。
- 读取或修改游戏内存。
- 自动点击、自动选择、自动重随或自动确认。
- 模拟键鼠输入。
- 默认枚举进程或窗口标题。
- 样本特化 OCR 错字表。
- 伪造 ApexLOL 数据。

默认目标显示模式是无边框；独占全屏只能作为风险项和诊断项。

## 开发流程

每个阶段开始前先说明目标、输入、输出和验收方式。修改代码或文档前先检查 `git status`，不要覆盖用户未提交改动。

阶段验证命令：

```bash
git diff --check
mise exec -- npm run build
mise exec -- cargo check --manifest-path src-tauri/Cargo.toml
```

涉及具体功能时，还要运行该阶段对应的功能验证或离线回放验证。阶段完成后使用中文提交；如果配置了 remote，验证通过后 push。

- 只要本轮修改了代码，默认还必须重新构建一份可交付的 Windows 安装包给用户测试，除非用户明确说明这轮不需要安装包。
- 构建 Windows 安装包后，答复中必须明确给出安装包产物路径和本轮实际执行的构建命令。

### Windows 安装包与排障包

- 正式给用户的默认 Windows 安装包必须使用不带排障开关的命令构建：

```bash
mise exec -- npm run build:windows
```

- 排障模式通过 Rust 编译开关 `debug-diagnostics` 显式开启，只用于本地问题定位或让用户复现疑难问题时单独出包，默认发版必须关闭该开关。
- 本地调试排障模式可使用：

```bash
mise exec -- npm run tauri:debug-diagnostics
```

- 构建 Windows 排障包可使用：

```bash
mise exec -- npm run build:windows:debug
```

- 仅检查 Rust 排障模式是否可编译可使用：

```bash
mise exec -- npm run check:rust:debug
```

- 排障模式开启后，允许额外输出高噪声诊断信息，例如 `runtime-panel-diagnostic-*.json` 和 `overlay-debug.log`；正式包默认不应包含这些高频调试落盘行为。
- 后续若线上问题需要重新排查，应优先在最新代码基础上开启 `debug-diagnostics` 重出排障包，不要依赖长期漂移的专用排障分支。

## 代码组织

- Rust 侧按职责拆模块，不把所有 Tauri command 堆在一个文件。
- 前端只负责交互和展示，截图、OCR、查询、缓存、状态机尽量放 Rust 层。
- 每个关键模块都要写结构化日志，包含阶段、输入摘要、输出摘要、耗时、错误码和可读错误信息。
- 诊断文件写入应用数据目录下清晰子目录：`logs`、`samples`、`ocr-replay`、`captures`、`reports`、`cache`、`config`、`calibration`。
- 用户可见错误必须能在日志里定位到同一个错误码或 trace id。
