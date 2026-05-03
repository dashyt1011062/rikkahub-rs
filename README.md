# RikkaHub Web Rust

这是 RikkaHub Web 的 Rust 后端重写版，用低内存 Rust 服务替代原 JVM 后端，并复用现有的 Web UI 静态资源。

当前 VPS 上的生产入口：

```text
https://rikkahub.wuji.us.eu.org
```

当前服务监听：

```text
127.0.0.1:8091
```

## 当前状态

Rust 版已作为当前主服务运行，原 Kotlin/JVM 版不再作为生产服务使用。

已支持的主要功能：

- 登录鉴权和多账户。
- 会话列表、分页、搜索、详情和 SSE 更新。
- 发送消息、停止生成、重试、编辑、删除、分叉、置顶、移动会话。
- OpenAI 兼容、Google、Anthropic 风格供应商配置。
- 供应商模型获取、模型测试、模型能力配置。
- 默认模型与标题总结提示词配置。
- 自动标题总结和手动重新生成标题。
- MCP 服务器配置、工具同步和工具调用。
- 图片上传、图片消息、远程图片代理展示。
- 生图结果保存到远程图床。
- 备份导出和导入。
- 静态 Web UI 托管。

暂未重点迁移或仍可继续完善的部分：

- 高级设置、Memory、Prompt、Assistant 行为等深层配置。
- 与原版 RikkaHub 的全部边缘功能完全对齐。
- 前端仍以构建后的静态资源为主，后续如要长期维护，建议恢复或重建前端源码工程。

## 数据目录

默认生产数据目录：

```text
/opt/rikkahub/data
```

主要文件：

```text
/opt/rikkahub/data/rikka_hub.db
/opt/rikkahub/data/settings.json
/opt/rikkahub/data/accounts/<account>/settings.json
```

数据库使用 SQLite。会话、消息、文件记录、搜索索引等结构化数据存储在数据库中。

图片默认不落盘保存到 VPS，本地数据库只保存文件记录和远程地址。前端展示图片时优先通过当前 VPS 的代理接口流式转发，避免直接暴露或回退到带水印直链。

## 环境变量

参考 `.env.example`：

```env
HOST=0.0.0.0
PORT=8080
DATA_DIR=/data
WEB_UI_DIR=/web-ui
JWT_ENABLED=true
ACCESS_PASSWORD=change-me
WEB_ACCOUNTS=1953939569:change-me
UPLOAD_MAX_MB=100
FILE_STORAGE=imgpile
IMGPILE_KEY=
```

常用变量说明：

- `HOST` / `PORT`：服务监听地址和端口。
- `DATA_DIR`：数据目录。
- `DB_PATH`：可选，覆盖默认数据库路径。
- `WEB_UI_DIR`：静态前端目录。
- `JWT_ENABLED`：是否启用登录鉴权。
- `ACCESS_PASSWORD`：默认账户密码，对应默认账户 `2819915628`。
- `WEB_ACCOUNTS`：额外账户，格式为 `username:password`，可用逗号、分号或换行分隔。
- `UPLOAD_MAX_MB`：上传文件大小限制。
- `FILE_STORAGE`：文件存储后端，当前生产使用 `imgpile`。
- `IMGPILE_KEY`：Imgpile API Key。
- `MAX_REMOTE_FILE_PROXIES`：远程图片代理并发数，默认 `10`。
- `MAX_REMOTE_FILE_PROXY_MB`：单个远程代理文件大小限制，默认 `64`。

不要把真实 `.env`、API Key、数据库、上传目录或备份文件提交到 Git。

## Docker 部署

仓库内提供 `docker-compose.yml`：

```bash
cd /opt/rikkahub-rs
docker compose up --build -d
```

Compose 默认映射：

```text
127.0.0.1:8091 -> container:8080
```

默认挂载：

```text
/opt/rikkahub/data -> /data
/opt/rikkahub-rs/web-ui -> /web-ui:ro
```

当前 VPS 生产容器名：

```text
rikkahub-rs-preview
```

当前 VPS 使用 Cloudflare 反代到本机 `127.0.0.1:8091`，公网只保留：

```text
https://rikkahub.wuji.us.eu.org
```

## 本地开发

直接运行：

```bash
cd /opt/rikkahub-rs
cargo run
```

检查和构建：

```bash
cargo check
cargo build
cargo build --release
```

如果需要使用 Docker 构建：

```bash
docker compose build
```

## 代码结构

```text
src/
  auth.rs              登录和 JWT 校验
  config.rs            环境变量配置
  db.rs                SQLite 读写和搜索索引
  engine.rs            会话写入、生成流程、标题生成调度
  llm.rs               模型请求、流式生成、工具调用适配
  mcp.rs               MCP 工具同步和调用
  routes/              HTTP API 路由
  imgpile.rs           Imgpile 上传适配
web-ui/                静态前端构建产物
```

## 维护注意

- 保留 Rust/Cargo 和 Docker 构建缓存可以加速后续构建。
- 清理磁盘时不要误删 `/opt/rikkahub/data`。
- 图片代理是流式转发，并限制并发，避免大量图片会话拖高内存。
- 修改供应商配置页后需要点击保存；模型列表里的模型能力设置会自动保存。
- GitHub 仓库只保存源码和静态资源，不保存真实密钥和运行数据。
