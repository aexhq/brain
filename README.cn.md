<pre align="center">
              ______ ______ _______ _______ _______
  ▄████▄     |   __ \   __ \   _   |_     _|    |  |
▄██▄██▄██▄   |   __ <      <       |_|   |_|       |
  ▀▀  ▀▀     |______/___|__|___|___|_______|__|____|
</pre>

<p align="center"><strong>按你的想法构建智能体，跨系统协同工作。</strong></p>

Brain 是一个开源的智能体运行时，支持可插拔的 agent loop、模型、工具和执行环境。
你可以使用现成的 loop，也可以编写自己的上下文管理和工具调用逻辑。
工具可以运行在应用、浏览器或远程环境中，应用通过统一的 API 与智能体交互。

[快速开始](https://aex.dev/brain/docs/quickstart) · [文档](https://aex.dev/brain/docs) ·
[官方扩展](https://github.com/aexhq/extensions) · [English](README.md)

例如，客服智能体可以通过你提供的工具查询后端订单、在浏览器中查看配送页面，再更新工单。
每个工具都在能够访问所需资源的位置运行。

- **选择智能体的行为。** 使用现成的 loop，或通过相同的公共接口实现自己的上下文选择、
  模型调用和工具调度逻辑。
- **选择执行位置。** 分别指定 loop 和工具在哪里运行。通过受支持的扩展连接应用函数、
  HTTP 服务、浏览器和远程计算环境。
- **接入你的应用。** 通过 Brain API 提交任务、查看进度和获取结果。可以自行运行 Brain，
  也可以使用 [Aex](https://aex.dev/docs) 托管。

## 快速开始

[快速开始指南](https://aex.dev/brain/docs/quickstart) 用一个应用函数演示订单查询。
指南包含服务器启动命令、配套依赖、完整的 TypeScript 示例和预期输出。
需要 Docker、Node.js 22 或更新版本，以及 OpenAI API key。

| 接下来要做什么 | 指南 |
| --- | --- |
| 流式显示回答、提交任务或重新连接 | [会话](https://aex.dev/brain/docs/concepts/sessions) |
| 接入应用函数或发布工具包 | [工具](https://aex.dev/brain/docs/guides/write-a-tool) |
| 自定义上下文和工具调用逻辑 | [Agent loop](https://aex.dev/brain/docs/guides/write-a-loop) |
| 接入浏览器和远程执行环境 | [环境](https://aex.dev/brain/docs/concepts/environment) |
| 配置服务器 | [配置参考](https://aex.dev/brain/docs/reference/configuration) |

## 什么时候使用 Brain

Brain 是应用连接的独立服务。如果单个应用进程已经能够管理整个智能体，嵌入式 agent 库可能就足够了。
使用 Brain 时，你可以在统一的会话 API 背后选择 loop 和各个工具的执行位置。

沙箱是隔离的代码执行环境，适合需要这种能力的工具；使用 Brain 并不要求沙箱。
普通应用函数可以直接作为工具。当 loop 在工具的沙箱之外运行时，它可以收到环境故障事件，
并自行决定如何处理。其他框架和托管服务也提供扩展能力与持久化会话；Brain 提供可独立运行的引擎，
并通过公共接口让你替换智能体行为和执行集成。

> **早期预览。** 1.0 之前 API 可能变化。存储完好时，已提交的历史记录可在服务器重启后保留；
> 被中断的工作会标记为失败，不会自动恢复或重试。保留会话历史不代表能恢复环境中丢失的文件或进程。

应用 SDK 支持 TypeScript 和 JavaScript。其他客户端可以使用
[HTTP API](https://aex.dev/brain/docs/reference/api)；扩展指南分别说明支持的语言和运行条件。

[贡献指南](CONTRIBUTING.md) · [设计决策](references/adrs/README.md) ·
[性能测试](BENCHMARKS.md) · [安全策略](SECURITY.md) · [MIT 许可证](LICENSE)

[提交问题](https://github.com/aexhq/brain/issues)，或联系 [support@aex.dev](mailto:support@aex.dev)。
