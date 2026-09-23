# Guche WebUI 功能说明

## 概述

Guche WebUI 是固车拾取工具的Web管理界面，提供实时监控、配置管理、群组管理等功能。

## 访问地址

启动程序后，WebUI会自动绑定到本地端口（默认8080），访问地址会在控制台显示：

```
WebUI 启动，访问地址: http://127.0.0.1:8080
```

## 功能模块

### 1. 运行状态监控
- WebSocket连接状态
- 启用群组数量
- 今日改名次数
- 总改名次数
- 日志缓冲使用情况
- WebUI端口信息

### 2. 配置管理
- 查看当前配置
- 编辑配置（即时生效）
- 重置为默认配置
- 配置验证

### 3. 群组管理
- 查看启用群组列表
- 添加群组
- 删除群组
- 查看群组详情（匹配模式、位置设置）

### 4. 统计查看
- 按群组查询统计
- 导出统计报告
- 清理过期数据

### 5. 实时日志
- WebSocket实时日志流
- 日志筛选（INFO/WARN/ERROR）
- 日志搜索
- 自动滚动控制

## API 端点

### 配置管理
- `GET /api/config` - 获取配置
- `PUT /api/config` - 更新配置
- `POST /api/config/reset` - 重置配置

### 群组管理
- `GET /api/groups` - 获取群组列表
- `POST /api/groups` - 添加群组
- `DELETE /api/groups/:id` - 删除群组

### 统计查看
- `GET /api/stats` - 获取统计（支持筛选）
- `GET /api/stats/:group_id` - 获取群组统计
- `DELETE /api/stats/:group_id` - 清理群组统计
- `GET /api/stats/export` - 导出统计报告

### 日志查看
- `GET /api/logs` - 获取日志（分页）
- `WS /api/logs/stream` - 实时日志流

### 状态监控
- `GET /api/status` - 获取运行状态

## 配置说明

WebUI配置在 `config.json` 中的 `webui` 字段：

```json
{
  "webui": {
    "enabled": true,
    "port": 8080
  }
}
```

- `enabled`: 是否启用WebUI（默认true）
- `port`: WebUI端口（默认8080，自动查找可用端口）

## 内存管理

- **日志缓冲**：1024条环形缓冲，淘汰最旧记录
- **统计数据**：每群最多1024条记录，磁盘化存储
- **消息队列**：512条有界channel，积压时丢弃旧消息
- **连接池**：10个连接，超时清理(5分钟) + 心跳检测(30秒)

## 端口管理

- **本地访问**：绑定127.0.0.1，不暴露到公网
- **自动找端口**：尝试8080-8085，失败则随机分配
- **SO_REUSEADDR**：允许重用TIME_WAIT状态的端口
- **优雅退出**：捕获退出信号，显式释放端口

## 安全特性

- 绑定127.0.0.1，仅本地访问
- 配置验证，防止非法配置
- 输入验证，防止注入攻击
- 错误处理，防止信息泄露

## 使用示例

### 1. 查看运行状态
```bash
curl http://127.0.0.1:8080/api/status
```

### 2. 修改配置
```bash
curl -X PUT http://127.0.0.1:8080/api/config \
  -H "Content-Type: application/json" \
  -d '{"guche": {"match_mode": "fuzzy"}}'
```

### 3. 添加群组
```bash
curl -X POST http://127.0.0.1:8080/api/groups \
  -H "Content-Type: application/json" \
  -d "123456789"
```

### 4. 查看实时日志
```bash
# 使用WebSocket客户端连接
ws://127.0.0.1:8080/api/logs/stream
```

## 故障排除

### 1. 端口被占用
程序会自动尝试其他端口，如果所有端口都被占用，会使用随机端口。

### 2. 无法访问WebUI
- 检查程序是否正在运行
- 检查控制台输出的端口号
- 检查防火墙设置
- 确认配置中webui.enabled为true

### 3. 配置修改不生效
- 配置修改会立即生效
- 如果使用了热重载，检查配置文件语法是否正确

## 开发说明

### 技术栈
- **后端**: Rust + axum + tokio
- **前端**: 原生HTML/CSS/JavaScript
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