# OCR 模型资源

当前 OCR 基础模块预留 PP-OCRv4 rec ONNX 接口，模型文件必须放在本目录：

```text
src-tauri/resources/models/ppocrv4_rec.onnx
```

模块启动推理前会先检查文件是否存在。文件缺失时不会静默降级，会返回包含完整路径的 `HEX-OCR-MODEL-MISSING` / `ModelMissing` 错误，便于定位资源问题。

当前实现范围：

- 使用 `ort` 建立 PP-OCRv4 rec ONNX 会话骨架。
- 模型缺失时返回可定位错误。
- OCR 图像预处理和 CTC 解码尚未接入，模型存在后会话加载成功，但文本识别接口仍返回 `InferenceNotImplemented`。
- 离线回放只处理三个校准名称区域，输出每个 slot 的原始文本、OCR 置信度、词库匹配分、最终名称、耗时和失败原因。

