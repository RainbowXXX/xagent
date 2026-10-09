# xagent

xagent 目前是一个可嵌入的 agent 核心框架。`src/lib.rs` 导出平台无关的组件；`src/main.rs` 只负责读取 CLI 和配置、启动应用。实际终端驱动和模型 API 适配器尚未接入。

## 运行链路

```text
FrontendDriver → RuntimeHandle
    │ submit / resume / continue_turn / abort_turn
    ▼
AgentRuntime → AgentLoop ── RuntimeExtension（上下文变换、事件观察）
    ├── SessionStore（会话与待执行工具调用）
    ├── ModelRegistry → ModelProvider
    ├── PermissionEngine（allow / deny / ask）
    └── ToolRegistry → Tool
                         ▲
                         └── McpManager + McpConnector
```

`boot` 读取配置并组装上述服务；`boot_with` 的 `BootOptions` 可在启动时注入自定义会话存储和权限引擎。模型通过 `provider/model` 标识选择。模型返回工具调用后，运行时保存调用，按权限策略执行、拒绝或返回 `ApprovalRequired`；前端取得用户决定后调用 `resume`。模型调用失败时，会话保持 `Running`，可用 `continue_turn` 重试。单个会话的调用串行执行，不同会话可并行。

Agent Loop 先处理已保存的工具调用，再构建上下文并调用模型。它会校验模型响应，记录工具调用及结果，并持续循环到获得最终回复、等待审批或出错。模型步数上限按整个轮次计数，暂停和重试不会重置；达到上限后可提高限制再调用 `continue_turn`。`abort_turn` 会等待当前运行调用释放会话锁，为仍未执行的工具调用写入错误结果，并结束该轮次；它不负责中断正在执行的模型或工具请求。

## 异步扩展

`RuntimeExtension::on_attach` 会收到可克隆的 `ExtensionContext`。扩展可以用 `spawn` 启动长期运行的任务，然后立即从 `on_attach` 返回。任务在未来被文件变化、计时器或其他事件唤醒时，可读取 `session` 状态，自行选择调用 `post_context`，或调用 `start_turn` 主动发起新轮次。`start_turn` 立即返回任务句柄，扩展可在后台等待结果，也可丢弃句柄让任务继续运行。Agent Loop 仍会等待 `before_model`、`after_model` 和 `on_event` 回调返回，所以回调内不要等待同一会话的 `start_turn` 任务。

`post_context` 不等待会话锁；Agent Loop 在下一处安全边界将内容以 `ExtensionInput` 写入上下文。如果内容在模型调用期间到达，Loop 会在收到模型回复后继续一轮，使下一次模型调用看到它。如果投递恰好发生在轮次完成之后，内容会留在内存队列中，等待下一次模型调用。扩展投递队列目前不跨进程持久化，长期任务本身也由扩展负责恢复。

## 扩展位置

- 实现 `ModelProvider` 并注册到 `App::models()`，接入新的模型服务。
- 实现 `Tool` 并注册到 `App::tools()`；MCP 工具由 `McpConnector` 发现，自动以 `服务器名/工具名` 注册。
- 实现 `SessionStore`，替换默认的内存会话存储。
- 实现 `PermissionEngine`，结合当前会话异步判断工具权限，并通过 `BootOptions` 注入。
- 实现 `RuntimeExtension`，异步调整模型上下文、处理模型结果或订阅运行事件。回调需要长期等待时，应启动后台任务并尽快返回。
- 需要后台任务的扩展在 `on_attach` 中使用 `ExtensionContext::spawn`，再通过非阻塞的 `post_context` 或 `start_turn` 与 Agent 通信。
- 实现 `FrontendDriver`，通过 `RuntimeHandle` 驱动交互界面；具体终端后端仍由 `UiBackend` 隔离。

当前 `App::run()` 需要先设置 `FrontendDriver`。OpenAI、Windows 终端和 MCP 协议连接器尚无具体实现。内存会话不会跨进程保留。持久化存储接入后，还需要考虑工具执行已产生副作用、但结果尚未保存时的重试去重。
