<pre align="center">
              ______ ______ _______ _______ _______
  ▄████▄     |   __ \   __ \   _   |_     _|    |  |
▄██▄██▄██▄   |   __ <      <       |_|   |_|       |
  ▀▀  ▀▀     |______/___|__|___|___|_______|__|____|
</pre>

<p align="center"><strong>按你的方式构建智能体，让工具在数据所在之处运行。</strong></p>

<p align="center">
<a href="https://aex.dev/brain/docs/quickstart">快速开始</a> ·
<a href="https://aex.dev/brain/docs">文档</a> ·
<a href="https://github.com/aexhq/extensions">官方扩展</a> ·
<a href="README.md">English</a>
</p>

Brain 是一个开源的智能体运行时。agent loop、模型、工具以及它们各自的运行位置都由你决定。
Brain 负责保存会话，并为你的应用提供统一的 API。

```ts
const lookupOrder = tool({
  name: "lookup_order",
  description: "Look up an order by id.",
  input: z.object({ id: z.string() }),
  run: ({ id }, ctx) => ctx.finish({ id, status: "shipped" }),
});

const session = await brain.sessions.create({
  model: { provider: "openai", name: "gpt-5-mini", apiKey },
  agentloop: pi(),
  tools: [lookupOrder()],
});
await session.send("Has order A-1001 shipped?");
```

loop 运行在 Brain 服务器上，`lookup_order` 则运行在你的应用中，紧挨着你自己的数据。

## 为什么选择 Brain

- **掌控智能体的行为。** 直接使用现成的 loop，或自己编写上下文管理、模型调用和工具调度逻辑。
  官方 loop 与你的代码使用同一套公共接口。
- **每个工具都在合适的位置运行。** 应用进程、用户的浏览器标签页、HTTPS 后端、浏览器自动化服务或远程沙箱，
  创建会话时可以为每个工具单独选择。
- **从应用中驱动智能体。** 通过统一的会话 API 发送消息、流式查看进度、提交后台任务并稍后获取结果。
- **自托管或托管，任你选择。** 在自己的基础设施上运行 MIT 许可的服务器，或交给
  [Aex](https://aex.dev/docs) 托管。

## 快速开始

[快速开始指南](https://aex.dev/brain/docs/quickstart)会运行一个通过应用函数查询订单的智能体。
你需要 Docker、Node.js 22 或更新版本，以及 OpenAI API key。

| 我想要… | 阅读 |
| --- | --- |
| 流式显示回答、提交任务或重新连接 | [会话](https://aex.dev/brain/docs/concepts/sessions) |
| 接入自己的函数或发布工具包 | [编写工具](https://aex.dev/brain/docs/guides/write-a-tool) |
| 改变智能体管理上下文和调用工具的方式 | [编写 loop](https://aex.dev/brain/docs/guides/write-a-loop) |
| 使用浏览器和远程执行环境 | [环境](https://aex.dev/brain/docs/concepts/environment) |
| 配置自托管服务器 | [配置参考](https://aex.dev/brain/docs/reference/configuration) |

在应用启动时调用 `await brain.prepare(placed)`，即可预先准备 loop 和工具代码，并在多个会话间复用。
详见[环境预备](https://aex.dev/brain/docs/reference/environment-runtime#prepare-before-creating-sessions)。

## 什么时候使用 Brain

Brain 是应用所连接的独立服务。如果单个进程就能管理整个智能体，嵌入式 agent 库可能更简单。
当你希望替换 loop、把工具分布到多个系统，并让会话拥有独立的 API 时，就适合使用 Brain。

使用 Brain 不需要沙箱：普通应用函数就能作为工具。当某个工具确实需要沙箱时，loop 运行在沙箱之外，
因此沙箱故障会作为观测结果交给 loop 处理。

其他框架和托管服务也提供扩展能力与持久化会话；Brain 的特点是可独立运行的引擎，
加上可替换智能体行为和执行集成的公共接口。

> **早期预览。** 1.0 之前 API 可能变化。存储完好时，已保存的历史记录可在重启后保留。
> 被中断的工作会标记为失败，不会自动恢复或重试；会话历史也无法恢复环境中丢失的文件或进程。

应用 SDK 支持 TypeScript 和 JavaScript。其他客户端可以使用
[HTTP API](https://aex.dev/brain/docs/reference/api)。各扩展指南会列出其支持的语言。

## 项目

[贡献指南](CONTRIBUTING.md) · [设计决策](references/adrs/README.md) ·
[性能测试](BENCHMARKS.md) · [安全策略](SECURITY.md) · [MIT 许可证](LICENSE)

发现问题？[提交 issue](https://github.com/aexhq/brain/issues)，或发送邮件至
[support@aex.dev](mailto:support@aex.dev)。
