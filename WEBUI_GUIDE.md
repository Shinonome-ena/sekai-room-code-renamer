# WebUI 使用指南

## 快速开始

### 1. 启动WebUI

启动guche程序后，WebUI会自动启动：

```bash
cargo run --manifest-path crates/guche-app/Cargo.toml
```

启动成功后会显示：
```
WebUI 启用，端口: 8080
WebUI 启动，访问地址: http://127.0.0.1:8080
```

### 2. 访问WebUI

在浏览器中打开：`http://127.0.0.1:8080`

## 功能说明

### 1. 状态监控

主页显示实时运行状态：
- WebSocket连接状态
- 启用群组数量
- 今日改名次数
- 总改名次数
- 日志缓冲使用情况
- WebUI端口信息

### 2. 群组管理

**查看群组列表：**
- 点击"群组管理"标签
- 查看所有启用的群组
- 查看每个群组的匹配模式和位置设置

**添加群组：**
1. 在"添加群组"区域输入群组ID
2. 点击"添加"按钮
3. 群组将立即启用

**删除群组：**
1. 在群组列表中找到要删除的群组
2. 点击"删除"按钮
3. 确认删除

### 3. 配置管理

**查看配置：**
- 点击"配置编辑"标签
- 查看当前配置参数

**修改配置：**
1. 修改需要的配置项
2. 点击"保存配置"
3. 配置立即生效

**重置配置：**
- 点击快捷操作区域的"重置配置"按钮
- 配置将重置为默认值

### 4. 实时日志

**查看日志：**
- 点击"实时日志"标签
- 查看实时日志流
- 支持自动滚动和手动滚动

**日志筛选：**
- 日志按时间顺序显示
- 不同级别用不同颜色标识：
  - INFO: 绿色
  - WARN: 黄色
  - ERROR: 红色

**清空日志：**
- 点击"清空日志"按钮

### 5. 统计查看

**查看统计：**
- 点击"统计详情"标签
- 查看每个群组的改名统计
- 查看最后改名时间

**清理统计：**
- 点击快捷操作区域的"清理所有统计"按钮
- 清理所有统计数据

## API 接口

WebUI提供完整的REST API接口：

### 状态查询
```bash
GET /api/status
```

### 配置管理
```bash
GET /api/config          # 获取配置
PUT /api/config          # 更新配置
POST /api/config/reset   # 重置配置
```

### 群组管理
```bash
GET /api/groups          # 获取群组列表
POST /api/groups         # 添加群组
DELETE /api/groups/:id   # 删除群组
```

### 统计查询
```bash
GET /api/stats           # 获取统计
GET /api/stats/:group_id # 获取群组统计
GET /api/stats/export    # 导出统计报告
```

### 日志查询
```bash
GET /api/logs            # 获取日志
WS /api/logs/stream      # 实时日志流
```

## 配置说明

WebUI配置在 `config.json` 中：

```json
{
  "webui": {
    "enabled": true,
    "port": 8080
  }
}
```

- `enabled`: 是否启用WebUI（默认true）
- `port`: WebUI端口（默认8080）

## 故障排除

### 1. 无法访问WebUI

**检查步骤：**
1. 确认程序正在运行
2. 检查控制台输出的端口号
3. 检查防火墙设置
4. 确认配置中 `webui.enabled` 为 `true`

**常见问题：**
- 端口被占用：程序会自动尝试其他端口
- 防火墙阻止：需要允许本地访问

### 2. 配置修改不生效

**解决方法：**
1. 配置修改会立即生效
2. 如果使用了热重载，检查配置文件语法
3. 重启程序

### 3. WebSocket连接失败

**解决方法：**
1. 检查浏览器是否支持WebSocket
2. 检查网络连接
3. 刷新页面重试

## 高级功能

### 1. 实时监控

WebSocket实时日志流支持：
- 实时日志推送
- 自动重连
- 心跳检测

### 2. 连接池管理

自动管理WebSocket连接：
- 最大10个连接
- 5分钟超时清理
- 30秒心跳检测

### 3. 内存管理

- 日志缓冲：1024条环形缓冲
- 消息队列：512条有界队列
- 自动清理过期数据

## 安全说明

WebUI仅绑定本地地址（127.0.0.1），不暴露到公网。如需远程访问：

1. 使用SSH隧道
2. 配置反向代理
3. 修改绑定地址（不推荐）

## 开发说明

### 技术栈
- **后端**: Rust + axum + tokio
- **前端**: HTML/CSS/JavaScript
- **通信**: REST API + WebSocket

### 文件结构
```
guche/
├── crates/guche-app/src/
│   ├── webui.rs          # WebUI核心逻辑
│   └── ...               # 其他模块
└── webui/
    └── index.html        # 前端界面
```

### 编译运行
```bash
cargo build --manifest-path crates/guche-app/Cargo.toml
cargo run --manifest-path crates/guche-app/Cargo.toml
```

## 更新日志

### v1.0.0
- 实现WebUI核心框架
- 实现所有API端点
- 实现实时日志流
- 实现内存管理和连接池
- 实现优雅退出机制
- 创建现代化前端界面