# Live Client 监听修复任务单

本文档用于在新会话中直接引用，指导修复“保存配置后开启监听进入游戏，但展示层没有展示”的问题。

## 背景

当前运行链路中，监听启动后展示层没有展示。已通过诊断包、结构化日志和接口探测脚本确认：

- 问题不在 OCR，也不在 overlay 自身渲染。
- 问题根因在 Live Client Data API 的接口选型和返回结构理解错误。
- 当前代码把 `/liveclientdata/activeplayer` 错当成了 `championName + level` 的来源。

## 已确认结论

以下结论已经通过宿主机日志和接口探测脚本确认，不需要再次猜测。

### 1. 当前错误用法

当前代码使用：

- `https://127.0.0.1:2999/liveclientdata/activeplayer`

并尝试直接解析：

- `championName`
- `level`

这是错误的。

### 2. 实测接口行为

在游戏运行时，以下接口可返回 `200`：

- `/liveclientdata/allgamedata`
- `/liveclientdata/activeplayer`
- `/liveclientdata/activeplayername`
- `/liveclientdata/playerlist`
- `/liveclientdata/gamestats`
- `/liveclientdata/eventdata`

### 3. 字段分布

实测结论：

- `activeplayer`
  - 返回当前操作者的能力、属性、金币等
  - **不包含** `championName`
  - 继续把它当成英雄名来源会导致 `HEX-LIVE-CLIENT-PAYLOAD`

- `activeplayername`
  - 返回当前操作者名称
  - 示例：`"loccen#65238"`

- `playerlist`
  - 返回玩家列表
  - 列表项中包含 `championName`

- `allgamedata`
  - 返回整包局内数据
  - 包含 `activePlayer`
  - 包含 `allPlayers`
  - 包含 `events`
  - 包含 `gameData`

### 4. 当前日志表现

旧版本监听无展示时，典型日志表现为：

- `HEX-LIVE-CLIENT-UNAVAILABLE`
- `HEX-LIVE-CLIENT-PAYLOAD`
- `panelState = collapsed`
- `pendingTiers = []`
- `toStatus = paused`

新加的定界日志已经能输出：

- 请求 URL
- HTTP 状态码
- 响应体长度
- 响应体前 120 字符摘要
- JSON 解析失败/缺字段原因
- 进入状态机前的输入摘要

## 修复目标

目标不是“继续加日志”，而是把监听链路切换到正确的数据来源，使运行时能够稳定拿到：

- 当前玩家名称
- 当前玩家英雄名
- 当前玩家等级
- 当前游戏模式
- 当前游戏时间

并且让状态机能够基于这些信息进入正常流转，而不是长期停在 `paused`。

## 推荐实现方案

### 阶段 1：重构 Live Client 数据抓取模型

不要再把 `ActivePlayerSnapshot` 绑定到 `/activeplayer`。

建议在 `src-tauri/src/live_client.rs` 中新增更合理的数据结构，例如：

```text
LiveClientResolvedPlayer
- active_player_name: String
- champion_name: String
- level: u8
- game_mode: Option<String>
- game_time: Option<f64>
- source: String
```

以及诊断结构，例如：

```text
LiveClientFetchDiagnostics
- requested_endpoints: Vec<String>
- activeplayername_status
- allgamedata_status
- playerlist_status
- fallback_used
- response_preview
```

### 阶段 2：接口组合策略

推荐策略如下：

#### 主路径

1. 请求 `/liveclientdata/activeplayername`
   - 获取当前操作者名字，记为 `active_player_name`

2. 请求 `/liveclientdata/allgamedata`
   - 从中读取：
     - `allPlayers`
     - `activePlayer`
     - `events`
     - `gameData`

3. 在 `allPlayers` 中按当前玩家名字匹配当前玩家
   - 优先尝试字段：
     - `summonerName`
   - 如果存在其它命名字段，再根据实际结构补充

4. 从命中的玩家项提取：
   - `championName`
   - `level`

5. 从 `gameData` 提取：
   - `gameMode`
   - `gameTime`

#### 兜底路径

如果 `allgamedata.allPlayers` 缺失或找不到当前玩家：

1. 请求 `/liveclientdata/playerlist`
2. 继续按当前玩家名匹配
3. 提取 `championName`

注意：

- `/activeplayer` 仍可保留，用于读取金币、技能、属性等
- 但不再负责 `championName` 和 `level`

### 阶段 3：orchestrator 改造

在 `src-tauri/src/orchestrator.rs` 中：

- 把当前的
  - `fetch_active_player()`
- 替换成
  - `fetch_resolved_player_snapshot()`

状态机输入前，至少要记录：

- `activePlayerName`
- `resolvedChampionName`
- `resolvedLevel`
- `gameMode`
- `gameTime`
- `sourceEndpoint`
- `fallbackUsed`

### 阶段 4：模式过滤

探测结果显示当前模式值类似：

- `gameMode = "KIWI"`

实现时应：

- 在运行时入口加模式过滤
- 非目标模式直接暂停并打清晰日志
- 目标模式才继续走海克斯阶段判断

### 阶段 5：overlay 触发条件日志

即便这一步不作为主修复，也建议顺手补上，避免再次出现“上游没数据，但误以为是 overlay 没弹”。

建议新增这些结构化日志阶段：

- `live-client-fetch-start`
- `live-client-fetch-success`
- `live-client-fetch-failed`
- `live-client-player-resolve-start`
- `live-client-player-resolve-success`
- `live-client-player-resolve-failed`
- `overlay-trigger-check`
- `overlay-trigger-ready`
- `overlay-trigger-skipped`

`overlay-trigger-skipped` 至少应包含：

- 当前模式
- 当前面板状态
- `pendingTiers`
- `pauseReason`
- 是否存在待展示 choices

## 推荐修改文件

- `src-tauri/src/live_client.rs`
  - 重新设计数据抓取与解析入口

- `src-tauri/src/orchestrator.rs`
  - 改状态机输入来源
  - 增强监听与展示触发日志

- 如有必要：
  - `src-tauri/src/models.rs`
  - `src-tauri/src/commands.rs`

## 验收标准

### 功能验收

在标准 Windows 安装包运行、进入目标对局后：

- 不再出现把 `/activeplayer` 当成 `championName` 来源的问题
- `runtime-orchestrator` 日志里能看到：
  - 当前玩家名
  - 当前英雄名
  - 当前等级
  - 当前模式
- 运行时不再长期停在：
  - `paused`
  - `pendingTiers = []`
  - `panelState = collapsed`

### 日志验收

`app.jsonl` 中能够清楚区分：

- 请求失败
- HTTP 非 200
- JSON 结构缺失
- 无法在 `allPlayers` 中定位当前玩家
- 模式不匹配
- overlay 条件未满足

### 建议验证命令

```bash
git diff --check
mise exec -- cargo check --manifest-path src-tauri/Cargo.toml
mise exec -- npm run build
mise exec -- npm run build:windows
```

如新增解析函数，补最小单测，至少覆盖：

- `activeplayer` 不含 `championName` 时不再误判为正确结构
- `activeplayername + allgamedata` 能解析出当前玩家
- `playerlist` 兜底逻辑可用

## 额外说明

当前仓库里与本问题相关的两个修复已经合入主线：

- 标题自动精裁
- Live Client 定界日志

因此后续修复必须基于 `main`，不要再从旧悬空分支继续实现。
