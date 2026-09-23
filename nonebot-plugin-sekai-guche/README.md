# nonebot-plugin-sekai-guche

《世界计划：缤纷舞台！》/《Project SEKAI: Colorful Stage!》固车群**自动拾取车牌改名** · NoneBot2 插件。

群内有人发出 5 位房号时，机器人自动把车牌写进群名，方便固车群友快速上车。

- 协议：OneBot V11
- 核心逻辑与独立版 `guche-core` 对齐（严格/宽松、前缀/后缀、60 字截断、冷却、权限）

## 安装

**nb-cli（推荐）：**

```bash
nb plugin install nonebot-plugin-sekai-guche
```

**pip：**

```bash
pip install nonebot-plugin-sekai-guche
```

随后在 NoneBot 项目中加载本插件（并启用 OneBot V11 适配器）。

## 配置

通过 NoneBot 配置项（`.env` 等）设置，均可省略：

| 配置项 | 类型 | 默认 | 说明 |
|--------|------|------|------|
| `GUCHE_ADMIN_USERS` | `list[int]` | `[]` | 固车助手管理员列表（非群聊管理员；OneBot `user_id`）；**空 = 所有人都能用管理指令** |
| `GUCHE_FUZZY` | `bool` | `false` | 全局匹配：`false` 严格，`true` 宽松 |
| `GUCHE_POS_END` | `bool` | `false` | 全局位置：`false` 前缀，`true` 后缀 |
| `GUCHE_COOLDOWN_SECONDS` | `float` | `5.0` | 按群冷却（秒） |
| `GUCHE_ENABLE_CMD` | `str` | `/开启固车拾取` | 启用指令 |
| `GUCHE_DISABLE_CMD` | `str` | `/关闭固车拾取` | 关闭指令 |
| `GUCHE_HELP_CMD` | `str` | `/固车拾取 使用方法` | 帮助指令 |
| `GUCHE_MODE_CMD` | `str` | `/固车模式` | 模式指令 |

已启用群与每群的 `pos` / `mode` 覆盖会存放在本地数据文件（由 `nonebot-plugin-localstore` 管理）。

## 用法

| 发送 | 作用 | 权限 |
|------|------|------|
| `/开启固车拾取` | 启用本群 | 管理员 |
| `/关闭固车拾取` | 关闭本群（未启用时静默） | 管理员 |
| `/固车拾取 使用方法` | 帮助 | 所有人 |
| `/固车模式` | 查看/设置前后缀、严格宽松 | 管理员 |
| 5 位数字 | 自动写入群名 | 所有人（已启用群） |

示例：`/固车模式 后缀 宽松`（只改提到的维度）。

## 效果

- 消息 `12345` → 群名变为 `12345 原群名`（前缀）或 `原群名 12345`（后缀）
- 群名超过 60 字自动截断，保留车牌并提示

## 说明

- 仅支持 **OneBot V11**（`supported_adapters: ~onebot.v11`）
- 开源协议：AGPL-3.0-or-later
- 仓库内另有独立运行版（WebUI），见主仓库 README
