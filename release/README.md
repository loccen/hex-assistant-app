# 发布包说明

本文说明 LOL 海克斯助手发布包应包含的文件、Windows 打包命令、运行方式、已知限制、诊断包导出和验收记录要求。

当前仓库位于 `/home/code/hex-assistant-app`。旧 POC 仓库 `/home/code/hex-assistant` 只作为历史验证来源，不能作为正式发布目录。

## 1. 当前状态

截至本文档记录时，本仓库还不能在 WSL 中完成 Windows Overlay、真实游戏截图、点击穿透和端到端局内流程验收。原因是这些能力依赖真实 Windows 桌面、LOL 客户端、无边框游戏窗口、多显示器状态和 Windows 窗口管理行为。

因此，任何发布结论都必须按实际测试记录填写，不能把 WSL 下的构建或文档检查写成 Windows 局内验收已通过。

## 2. 发布目录应包含的内容

正式发布归档建议使用以下结构：

```text
release/
  README.md
  验证记录.md
  packages/
    Hex Assistant.exe 或安装包
    *.msi 或 *setup.exe
    checksums.txt
  models/
    ppocr-v4-rec.onnx
    ppocr-v4-rec.json 或等价字典 / 元数据文件
  runtime/
    onnxruntime.dll
    其他 ONNX Runtime 依赖库
  docs/
    运行说明.md
    已知限制.md
    诊断包导出说明.md
  diagnostics/
    示例诊断包或空目录说明
```

最低要求：

- 必须包含可执行文件或安装包。
- 必须包含 OCR 模型文件，不能依赖开发机上的 pip 包目录或源码相对路径。
- 必须包含 Windows 运行所需的 ORT 动态库，例如 `onnxruntime.dll`。
- 必须包含运行说明、已知限制、诊断包导出说明和验证记录。
- 必须记录构建机器、构建时间、提交号、产物路径和校验值。

如果某项资源暂未打入发布包，发布说明必须写成“缺失 / 待补齐”，不能写成可用。

## 3. Windows 打包命令

所有安装、构建、开发和打包命令都必须在正式项目仓库执行，并通过 `mise exec` 进入项目运行时。

在 Windows 终端进入仓库根目录后执行：

```bash
mise trust
mise install
mise exec -- npm install
mise exec -- npm run build
mise exec -- npm run tauri build
```

如需只验证 Rust 工程：

```bash
mise exec -- npm run check:rust
```

如需启动开发模式：

```bash
mise exec -- npm run tauri dev
```

打包产物通常位于：

```text
src-tauri/target/release/
src-tauri/target/release/bundle/
```

实际路径以 Tauri 构建日志为准。发布时应复制最终安装包、可执行文件、模型、ORT 动态库和说明文档到 `release/` 下的归档目录，并生成校验值。

## 4. 运行前检查

运行发布包前确认：

- Windows 桌面环境可用，不是在纯 WSL 或无桌面会话中运行。
- LOL 使用无边框模式。独占全屏不是 MVP 默认可用前提。
- 用户已完成首次校准，包含三张海克斯名称区域、三张卡片底部锚点和底部展示 / 隐藏按钮区域。
- 发布包能加载 OCR 模型和 `onnxruntime.dll`。
- 网络可访问 ApexLOL；不可访问时应显示“暂无数据”，不能伪造推荐。
- Overlay 已在当前显示器、分辨率和缩放下验收点击穿透。

## 5. 已知限制

- WSL 只能用于代码、文档、前端构建和部分静态检查，不能真实验收 Windows Overlay、点击穿透、游戏截图和 LOL 局内状态。
- 独占全屏可能出现黑屏、旧帧或 Overlay 不稳定，不作为 MVP 默认支持场景。
- 多显示器、Windows 缩放、游戏 UI 缩放、地图和画质会影响截图裁剪、OCR 和 Overlay 坐标，必须分别留样。
- OCR 模型和 ORT 动态库缺失时，应用应明确提示 OCR 不可用。
- ApexLOL 页面结构变化、断网或无数据时，应显示“暂无数据”并保留失败原因。
- 助手只展示信息，不自动点击、不自动选择、不自动重随、不读取游戏内存、不注入或 Hook 游戏进程。

## 6. 诊断包导出说明

诊断包用于定位截图、OCR、ApexLOL、Overlay 和状态机问题。导出时应包含：

```text
diagnostics-YYYYMMDD-HHMMSS/
  manifest.json
  logs/
    app.log
    state-machine.jsonl
    overlay.log
    apexlol.log
  capture/
    original.png
    crop-slot-1.png
    crop-slot-2.png
    crop-slot-3.png
    capture-meta.json
  ocr/
    ocr-result.json
    enhanced-slot-1.png
    enhanced-slot-2.png
    enhanced-slot-3.png
  overlay/
    overlay-position.json
    overlay-screenshot.png 或录屏说明
  apexlol/
    request-response-summary.json
    cache-hit.json
  validation/
    验证记录.md
```

`manifest.json` 至少记录：

- 应用版本和提交号。
- Windows 版本、显示器编号、分辨率、缩放。
- LOL 显示模式、地图、游戏 UI 缩放。
- 是否使用无边框模式。
- OCR 模型路径、ORT 动态库路径和加载结果。
- ApexLOL 请求结果。
- 导出时间和测试人。

诊断包不得保存敏感令牌，不得包含与本功能无关的个人信息。

## 7. 问题定位入口

截图问题优先检查：

- 目标显示器编号、分辨率、Windows 缩放和游戏显示模式。
- 原始截图是否黑屏。
- 连续截图哈希或时间戳是否提示旧帧。
- 校准坐标是否仍匹配当前分辨率和 UI 缩放。

OCR 问题优先检查：

- 三个名称区域裁剪图是否准确。
- 增强图是否可读。
- OCR raw 文本、置信度、匹配分和最终标准名称。
- 模型文件和 `onnxruntime.dll` 是否来自发布包。
- 低置信度结果是否被错误用于 ApexLOL 查询。

ApexLOL 问题优先检查：

- 查询使用的英雄名和海克斯标准名称。
- 请求 URL、HTTP 状态、耗时、解析结果和缓存命中状态。
- 失败时界面是否显示“暂无数据”。
- 是否错误复用了其他英雄或其他海克斯的缓存。

Overlay 问题优先检查：

- Overlay 是否绑定到正确显示器。
- 三张说明卡片坐标和尺寸是否来自当前校准锚点。
- 是否遮挡名称、选择按钮、重随按钮或底部按钮。
- Windows 下点击穿透、置顶、不抢焦点是否真实通过。
- 面板收起时是否隐藏，重新展开时是否恢复。

状态机问题优先检查：

- Live Client 返回的英雄、等级和时间戳。
- 3 / 7 / 11 / 15 级待处理队列。
- 面板展开、收起、完成候选和完成确认的状态迁移。
- 重随后是否只刷新变化的 slot。
- 异常状态是否暂停自动展示并等待人工处理。

## 8. 发布验收结论填写规则

`release/验证记录.md` 是发布结论的唯一记录入口。填写时必须遵守：

- 只记录真实执行过的命令、环境和结果。
- WSL 构建或文档检查不能替代 Windows 局内验收。
- 没有测试的项目写“未验收”。
- 发现问题但可继续发布时写“带问题通过”，并列出限制和处理方案。
- 不能把旧 POC 的结论写成本仓库已通过。
