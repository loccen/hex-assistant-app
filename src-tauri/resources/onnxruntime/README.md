# ONNX Runtime 动态库资源

本目录用于随 Tauri 发布包携带 ONNX Runtime 动态库。

Windows 发布包至少需要包含：

```text
src-tauri/resources/onnxruntime/onnxruntime.dll
```

如所选 ONNX Runtime 发行包还依赖其他 DLL，也必须一并放入本目录。发布前必须在干净 Windows 桌面环境确认应用从发布包资源目录加载动态库成功，不能依赖开发机上的 `ORT_DYLIB_PATH`、系统 PATH、pip 包目录或源码相对路径。

当前仓库只包含本说明文件，不包含真实 ORT 动态库。本说明用于让 Tauri resource 配置和 release 导出包明确记录缺失状态。
