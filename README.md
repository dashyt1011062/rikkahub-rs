# RikkaHub Web Rust Rewrite

这是 RikkaHub Web 的 Rust 重写目录，当前目标是用低内存 Rust 后端逐步替换现有 JVM 后端。

当前阶段是兼容骨架，不会影响正在 8090 运行的 Kotlin 版服务。Rust 版默认跑在 `127.0.0.1:8091`。

## 已实现

- `/api/system/health`
- `/api/system/info`
- `/api/auth/token`
- `/api/settings/stream`
- `/api/settings/replace`
- `/api/conversations`
- `/api/conversations/paged`
- `/api/conversations/search`
- `/api/conversations/{id}`
- `/api/conversations/{id}/stream`
- `/api/conversations/stream`
- `/api/files/id/{id}`，支持 `?proxy=1` 流式代理远程图片
- `/api/files/path/{path...}`，本地文件流式返回
- 静态前端目录托管

## 暂未实现

- 发送消息、重试、编辑、删除、标题生成等写入型会话逻辑
- 模型调用、SSE 生成流、MCP 工具执行
- 图片上传到图床和生图保存
- 供应商模型拉取和测试
- 备份导入导出

## 本机试跑

先复制当前前端构建产物：

```bash
docker cp rikkahub:/app/web-ui /opt/rikkahub-rs/web-ui
```

再启动 Rust 版：

```bash
cd /opt/rikkahub-rs
docker compose up --build
```

访问：

```text
http://127.0.0.1:8091
```

后续完整替换前，保持 Kotlin 版 `rikkahub` 容器继续运行在 `127.0.0.1:8090`。

