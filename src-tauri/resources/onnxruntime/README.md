# ONNX Runtime 动态库资源

本目录用于随 Tauri 发布包携带 ONNX Runtime 动态库。

Windows 发布包至少需要包含：

```text
src-tauri/resources/onnxruntime/onnxruntime.dll
```

如所选 ONNX Runtime 发行包还依赖其他 DLL，也必须一并放入本目录。发布前必须在干净 Windows 桌面环境确认应用从发布包资源目录加载动态库成功，不能依赖开发机上的 `ORT_DYLIB_PATH`、系统 PATH、pip 包目录或源码相对路径。

当前仓库已放入官方 Windows x64 ONNX Runtime 1.25.0 release 包中的以下文件：

```text
src-tauri/resources/onnxruntime/onnxruntime.dll
src-tauri/resources/onnxruntime/onnxruntime_providers_shared.dll
src-tauri/resources/onnxruntime/ONNXRUNTIME-LICENSE
src-tauri/resources/onnxruntime/ONNXRUNTIME-ThirdPartyNotices.txt
src-tauri/resources/onnxruntime/ONNXRUNTIME-VERSION
```

来源为官方 GitHub release：

```text
https://github.com/microsoft/onnxruntime/releases/download/v1.25.0/onnxruntime-win-x64-1.25.0.zip
```

WSL 构建只能确认文件随包导出，不能替代干净 Windows 桌面环境中的动态库加载验收。
