# Guche 开发规范文档

> 最后更新：2026-09-17
> 状态：核心功能已实现，待服务器部署测试

---

## 一、项目定位

guche 是一个 Rust 实现的 PJSK 固车拾取工具，**一份源码，两种用法**：

- **独立运行**：直接运行 `guche.exe` / `./guche`，通过 OneBot V11 反向 WS 与 OneBot V11 客户端通信，纯后台服务，日志输出到文件
- **NoneBot2 插件**：将 `__init__.py` + `guche_core.pyd`/`.so` 放入 `src/plugins/guche/`，由 NoneBot2 加载，通信由 NoneBot2 的 OneBot V11 adapter 处理

两种模式共享同一个 `config.json`，核心逻辑完全一致。

---

## 二、基础架构

### 2.1 技术栈

| 组件 | 选型 | 说明 |
|------|------|------|
| 语言 | Rust | - |
| 异步运行时 | tokio | - |
| WebSocket | tokio-tungstenite | 反向 WS 服务端 |
| Web 框架 | axum | WebUI 服务 |
| JSON | serde + serde_json | - |
| 正则 | regex | 5位数字匹配 |
| 时间 | std::time | 手动实现，无外部依赖 |
| 日志 | log + 自定义 DualLogger | 输出到 stderr 和文件 |

### 2.2 通信方式

**反向 WebSocket**：guche 作为 WS 服务端，OneBot V11 客户端主动连接过来。

```
┌──────────┐  反向 WS 连接   ┌──────────┐
│ OneBot V11│ ──────────────► │  guche   │
│          │  OneBot V11     │ (WS服务端)│
│          │  JSON/array     │          │
└──────────┘                 └──────────┘
```

- 一条 WS 连接同时处理事件接收和 API 调用
- 消息格式：`array`（消息段数组，非 CQ 码字符串）
- 监听端口：用户在配置文件中自行设置
- WS 路径：`/ws`（与 haruki 一致）
- 需要在 OneBot V11 客户端的 `websocketClients` 中添加一条指向 guche 的反向连接
- 断线自动重连（客户端重启后 guche 自动恢复连接）
- 重连策略：指数退避（1s → 2s → 4s → 8s → ...，上限 30s），连接成功后重置

### 2.3 项目结构

```
guche/
├── Cargo.toml                   # workspace 根
├── config.json                  # 共享配置模板
│
├── crates/
│   ├── guche-core/              # lib.rs — 纯逻辑库（无 I/O，无 tokio）
│   │   ├── Cargo.toml           # crate-type = ["cdylib", "rlib"]
│   │   └── src/
│   │       ├── lib.rs           # 导出 Matcher, compute_new_name 等
│   │       ├── matcher.rs       # 消息匹配（严格/宽松）
│   │       ├── rename.rs        # 群名更新计算
│   │       ├── permission.rs    # 权限判定
│   │       └── cooldown.rs      # 冷却判定
│   │
│   ├── guche-app/               # main.rs — 独立程序（纯后台服务）
│   │   ├── Cargo.toml           # 依赖 guche-core (rlib) + tokio + WS
│   │   └── src/
│   │       ├── main.rs          # 入口：加载配置、启动 WS + 事件循环
│   │       ├── config.rs        # 配置加载/保存
│   │       ├── onebot.rs        # OneBot V11 WS 服务端 + API 调用
│   │       ├── guche.rs         # 调用 guche-core，接入事件循环
│   │       ├── help.rs          # 帮助信息
│   │       ├── stats.rs         # 统计数据存储/查询
│   │       └── utils.rs         # 工具函数
│   │
│   
│
└── py/
    ├── __init__.py              # NoneBot2 插件入口（import guche_core）
    └── _plugin.py               # NoneBot2 适配层（on_command, on_message 等）
```

`guche-core` 是纯逻辑，不依赖 tokio、不依赖任何 I/O 库。`guche-app` 链接 `guche-core` 并加上 WS 通信。`py/` 下的 `__init__.py` import `guche_core.pyd`/`.so` 并用 NoneBot2 API 接入。

---

## 三、固车拾取模块

> **原则：核心逻辑忠实翻译原版 `guche.py`，不擅自改动。**
> 扩展功能单独标注，不影响原版行为。

### 3.1 原版行为（忠实翻译自 guche.py）

#### 懒加载设计

- **无群启用时**：仅响应 `/开启固车拾取` 命令，内存占用 ≈ 0
- **有群启用时**：加载消息监听
- **所有群关闭时**：卸载消息监听

#### 命令

| 命令 | 说明 | 权限 |
|------|------|------|
| `/开启固车拾取` | 启用本群功能 | superuser |
| `/关闭固车拾取` | 关闭本群功能 | superuser |
| `/固车模式` | 查看/设置匹配模式和群名位置 | superuser |
| `/固车拾取 使用方法` | 显示帮助信息 | 所有人 |
| 发送纯5位数字 | 自动更名群聊 | superuser |

引用回复（原版原文）：
- 开启：`已启动本群的自动拾取车牌功能，将自动使用车牌为群聊更名`
- 关闭：`固车拾取功能已关闭，下一次冲榜见~`
- 改名：`已将群聊名称从'{旧名}'改为'{新名}'`

#### 权限控制（superuser 路线）

原版优先级：
1. 插件单独配置的 superuser（原版在 `config/pjsk_notify.yaml` 的 `guche.superusers`）
2. NoneBot2 .env superuser
3. 都没有配置 → 全部人有权限

guche.exe 简化为：
1. 配置文件中的 `admin_users` 列表
2. 都没有配置 → 全部人有权限

#### 消息匹配（原版严格匹配）

原版正则：`^\d{5}$`

消息必须为**纯5位阿拉伯数字**，不含任何其他字符。`12345` 匹配，`12345a` 不匹配，` 12345` 不匹配。

#### 群名更新逻辑（原版）

原版使用两个正则：
- 匹配：`^\d{5}$`（判断消息是否为5位数字）
- 提取旧房号：`^(.*\s)?(\d{5})$`（判断群名末尾是否已有5位数字）

处理流程：
1. 调用 `get_group_info` 获取当前群名
2. 用 `_TAIL_FIVE_DIGIT_RE = r"^(.*\s)?(\d{5})$"` 检测群名**末尾**是否已有5位数字
   - 如果有：提取 `base_name`（去掉末尾空格+5位数字的部分），去掉尾部空格
   - 如果没有：`base_name = 整个旧群名`
3. 拼接：`new_name = "{base_name} {新房号}"`
   - 如果 base_name 为空：`new_name = "{新房号}"`
4. 调用 `set_group_name` 更新群名
5. 引用回复确认

原版行为示例：

| 当前群名 | 新车牌 | 结果 | 说明 |
|----------|--------|------|------|
| `娱乐群` | `66666` | `娱乐群 66666` | 无旧车牌，直接追加 |
| `娱乐群 12345` | `66666` | `娱乐群 66666` | 末尾有旧车牌，剥掉后追加新房号 |
| `娱乐群 12345 版本2` | `66666` | `娱乐群 12345 版本2 66666` | 末尾不是5位数字，直接追加 |

**原版始终将车牌放在群名末尾，不支持配置位置。**

#### 超长群名处理（原版）

- 群名最大 60 字符（`_GROUP_NAME_MAX_LEN = 60`）
- 超长时：截断 `base_name` 开头部分，保留房号
- 记录截断操作（保存到 `trimmed_name_records`，每群最多保留最近50条）

原版截断逻辑：
```
overflow = len(new_name) - 60
trimmed_base = base_name[overflow:]  // 从开头截掉多余部分
new_name = "{trimmed_base} {new_code}"
```

#### 冷却时间（原版）

- 5秒冷却，按群隔离
- 冷却数据存在内存中（通过 file_db 的 `cooldown` 字段）

#### 数据存储（原版）

原版使用 JSON 文件存储（`data/pjsk_notify/guche.json`）：
- `enabled_groups`：已启用群的列表（`[123456, 789012]`）
- `cooldown`：冷却时间戳（`{"guche_123456": 1725000000.0}`）
- `trimmed_name_records`：截断记录

---

### 3.2 扩展功能（guche.exe 新增）

以下功能在原版基础上扩展，不影响原版核心逻辑。

#### 匹配模式扩展

在原版严格匹配之外，增加宽松模式（配置项 `match_mode`）：

- `"strict"`（默认）：原版行为，`^\d{5}$`，纯5位数字
- `"fuzzy"`：宽松模式，`^(\d{5})`，从消息**开头**提取5位数字，后面允许有其他内容
  - `66666 🦐288+` → 提取 `66666`
  - `12345abc` → 提取 `12345`
  - `abc12345` → 不匹配（数字不在开头）

提取后，后续群名更新逻辑与原版完全一致（末尾追加，剥旧车牌）。

#### 群聊启停扩展

在原版群聊内命令之外，增加直接编辑配置文件的方式：

- **群聊内命令**（原版）：在目标群内发送 `/开启固车拾取` / `/关闭固车拾取`
- **编辑配置文件**（扩展）：直接编辑 `enabled_groups.json`（运行时数据）或通过群聊命令管理

两种方式操作的是同一个 `enabled_groups` 数据，互相同步。

#### 群名位置扩展

原版固定将车牌放在末尾。guche.exe 扩展支持配置群名位置：

- **全局设置**：`guche.group_position` 配置项，支持 `"start"`（前缀）和 `"end"`（后缀，默认）
- **单群覆盖**：`guche.group_overrides` 配置项，支持为特定群聊设置独立的位置
- **群内命令**：`/固车模式 前缀` 或 `/固车模式 后缀` 可切换位置设置

#### 匹配模式扩展

在原版严格匹配之外，增加宽松模式：

- **严格模式**（默认）：`^\d{5}$`，纯5位数字
- **宽松模式**：`^(\d{5})`，从消息开头提取5位数字，后面允许有其他内容
- **群内命令**：`/固车模式 严格` 或 `/固车模式 宽松` 可切换匹配模式

---

### 3.3 /固车拾取 使用方法（扩展）

原版无此命令。guche.exe 增加帮助命令，输出纯文本使用说明。

#### 权限

所有人可用。

---

## 四、OneBot V11 API 使用

guche 用到的 API（通过 WS 连接调用）：

| API | action | 用途 |
|-----|--------|------|
| 获取群信息 | `get_group_info` | 读取当前群名 |
| 设置群名 | `set_group_name` | 更新群名（追加/替换房号） |
| 发送群消息 | `send_group_msg` | 引用回复确认改名 |

事件接收：
- `message` 事件（`message_type: "group"`）：监听群消息

---

## 五、运行与日志

### 5.0 运行模式

纯后台服务，启动后自动运行：
- **Windows**：双击 `guche.exe`，窗口显示日志，关闭窗口即停止
- **Linux**：`./guche` 或 systemd 服务，日志输出到 stderr 和文件

### 5.1 日志

日志同时输出到两个目标：
- **stderr**：实时输出，Windows 双击运行时在窗口中可见
- **文件**：`guche.log`，持久化存储，供事后查看

日志格式（自定义 DualLogger）：
```
[1694841015000 INFO  guche] Guche 启动, 监听 127.0.0.1:8901
[1694841016000 INFO  guche::onebot] WS 服务监听 127.0.0.1:8901
[1694841020000 INFO  guche::onebot] 连接来自 127.0.0.1:54321
[1694841065000 INFO  guche::guche] 收到消息: 群123456 用户789 "12345"
[1694841065000 INFO  guche::guche] 匹配成功: 房号=12345
[1694841065000 INFO  guche::guche] 执行改名: 群123456 "娱乐群" → "娱乐群 12345"
[1694841065000 INFO  guche::guche] 已将群聊名称从'娱乐群'改为'娱乐群 12345'
```

格式：`[时间戳(毫秒) 级别 目标] 消息`

Linux 上实时查看日志：
```bash
tail -f guche.log
# 或 systemd 服务：
journalctl -u guche -f
```

### 5.2 配置方式

所有设置通过编辑 `config.json` 完成，修改后**热重载**生效（每2秒轮询配置文件变更）。

---

## 六、统计功能

### 6.1 数据存储

- 每群最多 1024 条更改记录
- 记录格式：时间、旧群名、新群名、用户ID
- 持久化到 JSON 文件（`data/stats.json`）

### 6.2 查询

- 直接查看 `stats.json` 文件
- 支持清理单群/全部群的指定日期前记录（编辑 JSON 文件）

### 6.3 今日统计

- 程序自动统计今日改名次数
- 可在日志中查看

---

## 七、帮助系统

- 默认命令：`/固车拾取 使用方法`
- 输出纯文本帮助信息（不生成图片，保持轻量）

---

## 八、配置文件

### 8.1 格式

JSON 格式，首次运行自动生成，支持热重载（每2秒轮询）。

### 8.2 配置结构

```json
{
  "first_run": true,
  "onebot": {
    "host": "127.0.0.1",
    "port": 8901,
    "token": null
  },
  "admin_users": [],
  "commands": {
    "guche_enable": "/开启固车拾取",
    "guche_disable": "/关闭固车拾取",
    "guche_help": "/固车拾取 使用方法",
    "guche_mode": "/固车模式"
  },
  "guche": {
    "match_mode": "strict",
    "group_position": "start",
    "group_overrides": {},
    "cooldown_seconds": 5
  },
  "stats": {
    "max_records_per_group": 1024
  }
}
```

字段说明：
- `host`：绑定地址，默认 `127.0.0.1`（仅本机，安全）。需远程访问时改为 `0.0.0.0` 或用 SSH 隧道
- `admin_users`：对应原版 `guche.superusers`，管理员列表
- `cooldown_seconds`：冷却时间，原版固定5秒
- `match_mode`：**扩展**，原版固定 `"strict"`
- `group_position`：**扩展**，群名位置，支持 `"start"`（前缀）和 `"end"`（后缀）
- `group_overrides`：**扩展**，单群位置覆盖，格式：`{"群号": "start/end"}`

---

## 九、数据文件

程序运行时产生以下文件（与可执行文件同目录）：

| 文件 | 内容 | 说明 |
|------|------|------|
| `config.json` | 配置 | 首次运行自动生成，手动编辑 |
| `data/enabled_groups.json` | 已启用群列表 | 程序自动维护 |
| `data/stats.json` | 改名统计记录 | 程序自动维护 |
| `data/guche.log` | 运行日志 | 持续追加，可定期清理 |

---

## 十、逆向防护

- strip 符号表（Cargo.toml 配置）
- 字符串加密（配置文件中的敏感信息）

---

## 十一、打包与发布

### 11.1 产物

每个平台发布一个压缩包，包含独立运行和 NoneBot2 插件两种用法所需的全部文件：

**Windows x86_64 示例**：

```
guche-v1.0-windows-x86_64.zip
├── guche.exe              ← 独立运行（双击或命令行）
├── guche_core.pyd         ← NoneBot2 插件用（Python import 的 Rust 核心）
├── __init__.py            ← NoneBot2 插件入口
├── config.json            ← 共享配置模板
└── README.md
```

**Linux x86_64 示例**：

```
guche-v1.0-linux-x86_64.tar.gz
├── guche                  ← 独立运行（./guche 或 systemd 服务）
├── guche_core.so          ← NoneBot2 插件用
├── __init__.py
├── config.json
└── README.md
```

**macOS ARM64 示例**：

```
guche-v1.0-macos-aarch64.tar.gz
├── guche                  ← 独立运行（./guche）
├── guche_core.so          ← NoneBot2 插件用
├── __init__.py
├── config.json
└── README.md
```

### 11.2 用户按需使用

| 场景 | 拿什么文件 | 放哪 |
|------|-----------|------|
| 我有 NoneBot2，当插件用 | `__init__.py` + `guche_core.pyd`/`.so` | `src/plugins/guche/` |
| 没有 NoneBot2，独立跑 | `guche.exe` / `guche` | 任意目录，直接运行 |

两种模式共享同一个 `config.json`，核心逻辑完全一致（`.pyd`/`.so` 和 `.exe` 是同一份 Rust 源码的不同编译产物）。

### 11.3 Rust 编译目标

`guche-core` crate 同时产出两个目标：

```toml
# crates/guche-core/Cargo.toml
[lib]
crate-type = ["cdylib", "rlib"]
```

- `cdylib` → 编译为 `.pyd`/`.so`，供 Python import
- `rlib` → 编译为静态库，供 `guche-app` 链接为独立二进制

### 11.4 编译选项

```toml
[profile.release]
opt-level = "z"
lto = true
codegen-units = 1
strip = true
```

预期产物大小：`guche.exe` / `guche` 约 1.8MB（含 tokio + WS + regex），`.pyd`/`.so` 约 100-200KB（不含 tokio runtime）。

> 依赖选型说明：保留 regex 库用于5位数字匹配和旧车牌提取，UTF-8 安全且与原版 Python 正则语义一致。虽然纯字符串操作可以替代，但需处理多字节字符边界，实现复杂度高且易出错。

### 11.5 GitHub Actions

- 触发：push/PR to main, 手动触发
- 编译矩阵：4 平台交叉编译，每个平台同时产出独立二进制 + cdylib
- 打包时将独立二进制 + cdylib + `__init__.py` + `config.json` 打入同一个压缩包
- 产物上传为 artifacts

---

## 十二、隐私保护

- 不暴露用户的配置信息
- 不导入开发机的开发者配置
- 示例中不包含真实账号 ID

---

## 十三、作者信息

- **作者**：Shinonome-ena
- **GitHub**：https://github.com/Shinonome-ena

---

## 十四、开发规范

### 14.1 开发顺序

1. **核心通信 + 固车拾取**（已完成）
   - OneBot V11 反向 WS 服务端（tokio-tungstenite）
   - 事件接收与 API 调用
   - 固车拾取核心逻辑（5位数字匹配 → 改群名 → 回复）
   - 配置文件加载（JSON）
   - 日志输出到 stderr 和文件
   - 目标：能在服务器上跑起来，群内发5位数字能改名

2. **统计 + 帮助 + 细节**（已完成）
   - 统计数据存储/查询
   - 帮助命令（纯文本）
   - 打磨细节

### 14.2 代码风格

- 能抽象复用的抽象
- 能一步做好的就不要拆成多个步骤
- 保持原版名称（如 ena 不叫东云绘名）
- 保持原版格式（综合力、实效等术语）

---

## 十五、与现有 NoneBot2 版 guche 的关系

guche.exe 是 NoneBot2 版 `pjsk_notify/guche.py` 的**独立替代品**。

- 部署时从 OneBot V11 客户端配置中移除旧版机器人的反向 WS 连接，改为连向 guche.exe
- 或者两者共存于不同群（旧版插件管一部分群，guche.exe 管另一部分群）
- 共存方案：客户端同时连接旧版机器人和 guche.exe，两者监听不同端口

---

## 十六、OneBot 服务端配置变更

guche.exe 运行后，需要在 OneBot V11 实现的**反向 WebSocket 客户端**配置中添加一条指向 guche 的连接。

首次运行时，程序自动使用默认配置启动，并在日志中输出 WS 地址提示：

```
首次运行，使用默认配置。编辑 config.json 修改设置。
端口: 8901，绑定: 127.0.0.1
OneBot V11 客户端需添加反向 WS: ws://127.0.0.1:8901/ws
```

### 客户端配置示例

在 `对应客户端的网络配置目录下 onebot11_{identifier}.json` 的 `network.websocketClients` 数组中添加：

```json
{
  "name": "guche",
  "enable": true,
  "url": "ws://127.0.0.1:{用户配置的端口}/ws",
  "messagePostFormat": "array",
  "reportSelfMessage": false,
  "reconnectInterval": 5000,
  "token": "",
  "debug": false,
  "heartInterval": 30000,
  "verifyCertificate": true
}
```

修改后需要重启对应服务（如 重启你的 OneBot V11 客户端）。

---

## 十七、万人级多租户服务架构（规划，暂不实施）

> 以下为未来扩展规划，当前版本不实现。

### 17.1 核心结论

guche-core 的核心逻辑（正则匹配 + 字符串拼接）单次调用微秒级，完全具备万人级并发性能。**不需要重新开发高性能核心**，现有 Rust 核心可直接内嵌复用。

瓶颈不在核心逻辑，在外围基础设施（多租户、存储、认证）。

### 17.2 架构设计

```
用户 OneBot 客户端 ──WS──► guche-platform
                       │
                  ┌────┴────┐
                  │ API网关  │  认证、限流、路由
                  └────┬────┘
                       │
            ┌──────────┼──────────┐
            ▼          ▼          ▼
      用户A的状态   用户B的状态   用户C的状态
      (群列表/配置) (群列表/配置) (群列表/配置)
            │          │          │
            └──────────┼──────────┘
                       ▼
                  guche-core（直接复用现有 Rust 库）
                       │
                       ▼
                    存储层（数据库）
```

### 17.3 需要新做的部分

| 组件 | 说明 |
|------|------|
| 多租户状态管理 | 每个会话账号独立的群列表、配置、冷却状态 |
| 并发 WS 管理 | 同时处理 N 个 OneBot V11 连接 |
| 存储层 | 数据库（SQLite/Redis）替代 JSON 文件 |
| 认证系统 | 每用户独立 token，申请/分发/吊销 |
| 限流/监控 | 防滥用、用量统计 |

### 17.4 可直接复用的部分

| 组件 | 复用方式 |
|------|----------|
| guche-core matcher | 直接调用 `match_message()` |
| guche-core rename | 直接调用 `compute_new_name()` |
| guche-core permission | 直接调用 `is_superuser()` |
| guche-core cooldown | 实例化 `Cooldown` per 群 |
| OneBot V11 协议处理 | 复用 onebot.rs 的 WS 帧解析 |

### 17.5 结论

核心逻辑零改动内嵌，外围基础设施是另一个量级的工程，定位为独立的平台项目。

---

## 十八、开发状态总结

### ✅ 已完成功能

1. **核心架构**
   - Rust 工作空间架构（guche-core、guche-app）
   - 模块化设计：核心逻辑与通信层分离
   - 编译配置：优化编译选项（strip、LTO、opt-level="z"）

2. **固车拾取模块**
   - OneBot V11 反向 WebSocket 服务端
   - 消息匹配：严格模式和宽松模式
   - 群名更新逻辑：前缀/后缀位置配置
   - 权限控制：superuser 权限管理
   - 冷却时间：5 秒冷却机制
   - 命令系统：开启/关闭/模式设置/帮助
   - 配置热重载：每 2 秒轮询配置文件变更
   - 数据持久化：启用群列表、统计记录

3. **NoneBot2 插件支持**
   - Python 适配层：`py/__init__.py` + `py/_plugin.py`
   - PyO3 绑定：`guche-core` 可编译为 `.pyd`/`.so`
   - 双模式运行：同一份核心逻辑支持独立运行和 NoneBot2 插件

4. **统计功能**
   - 改名记录存储：每群最多 1024 条记录
   - 记录格式：时间、旧群名、新群名、用户 ID
   - 今日统计：支持统计今日改名次数

5. **配置系统**
   - JSON 配置文件：支持注释、热重载
   - 首次运行引导：自动生成默认配置
   - 配置结构：完整的配置字段设计

6. **日志系统**
   - 双输出：stderr 实时输出 + 文件持久化
   - 日志格式：时间戳、日志级别、目标模块
   - 日志文件：`guche.log`

7. **编译产物**
   - 已成功编译：`guche.exe`、`guche_core.dll`
   - 运行时数据：程序已运行并产生日志文件

### 🔧 待完善功能

2. **内存管理优化**（开发完毕后补充）
   - 缓冲池设计：固定大小缓冲池
   - 低压清理：内存压力大时主动清理
   - 内存监控：日志中显示内存占用

3. **GitHub Actions**（待实现）
   - CI/CD 流水线：自动编译、测试、打包
   - 多平台构建：Linux x86_64/ARM64、Windows x86_64、macOS ARM64

### 📊 当前进度评估

- **核心功能完成度**：100%（固车拾取模块完全实现）
- **扩展功能完成度**：100%（匹配模式、群名位置、配置热重载等）
- **工具链完成度**：100%
- **文档完成度**：100%（开发规范文档）
- **测试覆盖度**：待评估

### 🎯 下一步工作

1. **服务器部署测试**（优先级高）
   - 在实际服务器环境测试
   - 验证与各类 OneBot V11 客户端的兼容性
   - 测试长时间运行的稳定性

2. **GitHub Actions 实现**（优先级中）
   - 自动化构建和测试
   - 多平台发布

3. **内存管理优化**（优先级低）
   - 开发完毕后补充
   - 实现缓冲池和低压清理

---

## 十九、附录：命令参考

### 固车拾取命令

| 命令 | 说明 | 权限 | 示例 |
|------|------|------|------|
| `/开启固车拾取` | 启用本群自动改名功能 | superuser | `/开启固车拾取` |
| `/关闭固车拾取` | 关闭本群自动改名功能 | superuser | `/关闭固车拾取` |
| `/固车模式` | 查看当前模式设置 | 所有人 | `/固车模式` |
| `/固车模式 前缀` | 设置车牌放在群名前面 | superuser | `/固车模式 前缀` |
| `/固车模式 后缀` | 设置车牌放在群名后面 | superuser | `/固车模式 后缀` |
| `/固车模式 严格` | 只识别纯5位数字 | superuser | `/固车模式 严格` |
| `/固车模式 宽松` | 消息开头提取5位数字 | superuser | `/固车模式 宽松` |
| `/固车模式 前缀 严格` | 同时设置位置和匹配模式 | superuser | `/固车模式 前缀 严格` |
| `/固车拾取 使用方法` | 显示帮助信息 | 所有人 | `/固车拾取 使用方法` |
| 发送5位数字 | 自动将数字添加到群名 | superuser | `12345` |

### 配置文件示例

```json
{
  "first_run": false,
  "onebot": {
    "host": "127.0.0.1",
    "port": 8901,
    "token": null
  },
  "admin_users": [100010001],
  "commands": {
    "guche_enable": "/开启固车拾取",
    "guche_disable": "/关闭固车拾取",
    "guche_help": "/固车拾取 使用方法",
    "guche_mode": "/固车模式"
  },
  "guche": {
    "match_mode": "strict",
    "group_position": "start",
    "group_overrides": {
      "123456789": "end"
    },
    "cooldown_seconds": 5
  },
  "stats": {
    "max_records_per_group": 1024
  }
}
```

---

## 二十、附录：技术细节

### 20.1 正则表达式

- **严格匹配**：`^\d{5}$`（纯5位数字）
- **宽松匹配**：`^(\d{5})`（从开头提取5位数字）
- **群名末尾检测**：`^(.*\s)?(\d{5})$`（检测末尾是否有5位数字）
- **群名开头检测**：`^(\d{5})\s+(.+)$`（检测开头是否有5位数字）

### 20.2 群名更新算法

```rust
// 前缀模式
if old_name starts with 5 digits:
    base_name = old_name[6..]  // 去掉 "12345 "
else if old_name is exactly 5 digits:
    base_name = ""
else:
    base_name = old_name

new_name = if base_name.is_empty() {
    new_code
} else {
    format!("{} {}", new_code, base_name)
}

// 后缀模式
if old_name ends with 5 digits:
    base_name = old_name[..len-5].trim_end()
else:
    base_name = old_name

new_name = if base_name.is_empty() {
    new_code
} else {
    format!("{} {}", base_name, new_code)
}
```

### 20.3 超长群名处理

```rust
const GROUP_NAME_MAX_LEN: usize = 60;

if new_name.chars().count() > GROUP_NAME_MAX_LEN {
    let overflow = new_name.chars().count() - GROUP_NAME_MAX_LEN;
    let base_char_count = base_name.chars().count();
    
    if base_char_count > overflow {
        // 前缀模式：从末尾截断
        // 后缀模式：从开头截断
        trimmed_base = base_name.chars().skip(overflow).collect();
        new_name = format!("{} {}", trimmed_base, new_code);
    } else {
        new_name = new_code.to_string();
    }
}
```

### 20.4 冷却时间实现

```rust
struct Cooldown {
    timestamps: HashMap<i64, f64>,  // group_id -> last_timestamp
}

impl Cooldown {
    fn check_and_update(&mut self, group_id: i64, now: f64, cooldown_secs: f64) -> bool {
        if let Some(&last) = self.timestamps.get(&group_id) {
            if now - last < cooldown_secs {
                return false;  // 冷却中
            }
        }
        self.timestamps.insert(group_id, now);
        true  // 可以执行
    }
}
```

### 20.5 WebSocket 消息格式

**接收消息**（OneBot V11 事件）：
```json
{
  "post_type": "message",
  "message_type": "group",
  "group_id": 123456789,
  "user_id": 987654321,
  "message_id": 12345,
  "message": [
    {
      "type": "text",
      "data": {
        "text": "12345"
      }
    }
  ]
}
```

**发送 API 调用**：
```json
{
  "action": "send_group_msg",
  "params": {
    "group_id": 123456789,
    "message": [
      {
        "type": "reply",
        "data": {
          "id": "12345"
        }
      },
      {
        "type": "text",
        "data": {
          "text": "已将群聊名称从'娱乐群'改为'娱乐群 12345'"
        }
      }
    ]
  }
}
```

---

## 二十一、WebUI 管理界面

### 21.1 设计原则

- **内嵌部署**：WebUI 与 guche-app 打包在一起，单文件部署
- **本地访问**：绑定 `127.0.0.1`，只允许本地访问，不暴露到公网
- **即时生效**：配置修改直接更新内存，同步持久化到 JSON 文件
- **优雅退出**：程序退出时自动释放端口，避免端口占用

### 21.2 技术选型

| 组件 | 选型 | 理由 |
|------|------|------|
| Web 框架 | axum | 性能好，与 tokio 集成好 |
| 前端框架 | 原生 HTML/CSS/JS | 轻量，无需构建工具 |
| API 格式 | JSON | 与现有配置格式一致 |
| 实时通信 | WebSocket | 日志实时推送 |

### 21.3 启动流程

1. 查找可用端口（尝试 8080-8085，失败则随机）
2. 绑定 `127.0.0.1:{port}`
3. 命令行显示访问地址
4. 启动 Web 服务器
5. 捕获退出信号，优雅关闭

### 21.4 功能模块

#### 配置管理
- 查看当前配置
- 编辑配置（即时生效）
- 重置为默认配置
- 验证配置合法性

#### 群组管理
- 查看启用群组列表
- 添加/删除群组
- 查看群组详情

#### 统计查看
- 按群组查询统计
- 按时间范围查询
- 导出活动统计报告
- 清理过期数据

#### 日志查看
- 实时日志流（WebSocket）
- 按类型筛选（RECV/MATCH/EXEC/REPLY/ERROR）
- 日志搜索

#### 状态监控
- WebSocket 连接状态
- 内存使用情况
- 启用群组数量
- 今日改名次数

### 21.5 API 设计

```
GET    /api/config              # 获取配置
PUT    /api/config              # 更新配置
POST   /api/config/reset        # 重置配置

GET    /api/groups              # 获取群组列表
POST   /api/groups              # 添加群组
DELETE /api/groups/:id          # 删除群组

GET    /api/stats               # 获取统计（支持筛选）
GET    /api/stats/:group_id     # 获取群组统计
DELETE /api/stats/:group_id     # 清理群组统计
GET    /api/stats/export        # 导出统计报告

GET    /api/logs                # 获取日志（分页）
WS     /api/logs/stream         # 实时日志流

GET    /api/status              # 获取运行状态
```

### 21.6 配置文件更新

```json
{
  "webui": {
    "enabled": true,
    "port": 8080
  }
}
```

### 21.7 内存管理

| 数据结构 | 限制 | 策略 |
|---------|------|------|
| 日志缓冲 | 1024 条 | 环形缓冲，淘汰最旧 |
| 统计数据 | 每群 1024 条 | 磁盘化存储，按群/时段清理 |
| 消息队列 | 512 条 | 有界 channel，积压时丢弃旧消息 |
| 连接池 | 10 个连接 | 超时清理(5分钟) + 心跳检测(30秒) |

### 21.8 端口管理

- **本地访问**：绑定 `127.0.0.1`，不暴露到公网
- **自动找端口**：尝试常用端口，失败则随机分配
- **SO_REUSEADDR**：允许重用 TIME_WAIT 状态的端口
- **优雅退出**：捕获退出信号，显式释放端口
- **RAII 模式**：异常退出时自动释放资源

---

## 二十二、待完善功能

### 22.1 WebUI（待开发）

- 前端界面开发
- API 实现
- 实时日志推送
- 统计图表展示

### 22.2 内存管理优化（开发完毕后补充）

- 缓冲池设计：固定大小缓冲池
- 低压清理：内存压力大时主动清理
- 内存监控：日志中显示内存占用

### 22.3 GitHub Actions（待实现）

- CI/CD 流水线：自动编译、测试、打包
- 多平台构建：Linux x86_64/ARM64、Windows x86_64、macOS ARM64

---

**文档结束**
