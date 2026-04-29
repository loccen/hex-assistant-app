# OCR 模型资源

当前 OCR 模块使用 PP-OCRv4 rec ONNX 做本地离线识别，模型文件必须放在本目录：

```text
src-tauri/resources/models/ppocrv4_rec.onnx
```

模块启动推理前会先检查文件是否存在。文件缺失时不会静默降级，会返回包含完整路径的 `HEX-OCR-MODEL-MISSING` / `ModelMissing` 错误；离线文本回放命令仍可继续用于验证词库匹配和报告结构。

当前仓库已放入本机 RapidOCR 安装包提供的 PP-OCRv4 rec ONNX 模型，并按正式资源路径命名为：

```text
src-tauri/resources/models/ppocrv4_rec.onnx
```

海克斯名称词库必须放在：

```text
src-tauri/resources/dictionaries/augments.zh-CN.json
```

PP-OCR 字符表优先从 ONNX 模型 metadata 的 `character` 字段读取。如果模型不带该 metadata，可额外放置：

```text
src-tauri/resources/dictionaries/ppocrv4_rec_chars.txt
```

该文件一行一个字符，不需要写 CTC blank；程序会自动补齐 blank 和空格。

当前实现范围：

- 使用 `ort` 加载 PP-OCRv4 rec ONNX 模型并执行推理。
- 将校准后的三个名称区域裁剪为 slot 图片，生成增强图，再送入 OCR。
- 预处理会把图片等比缩放到高度 48，按 PP-OCR rec 规则归一化到 `[-1, 1]`，输入张量形状为 `[1, 3, 48, W]`。
- 使用 CTC 贪心解码，blank 为 index 0，跳过连续重复字符。
- 报告会写入应用数据目录的 `reports/calibrated-name-slots-*` 子目录，包含裁剪图路径、增强图路径、原始文本、置信度、匹配分、最终名称、耗时和失败原因。
- 离线回放仍只处理三个校准名称区域文本输入，输出每个 slot 的原始文本、OCR 置信度、词库匹配分、最终名称、耗时和失败原因。
