<pre align="center">
              ______ ______ _______ _______ _______
  ▄████▄     |   __ \   __ \   _   |_     _|    |  |
▄██▄██▄██▄   |   __ <      <       |_|   |_|       |
  ▀▀  ▀▀     |______/___|__|___|___|_______|__|____|
</pre>

<p align="center"><strong>运行 AI agent，保存对话与工作进度。</strong></p>

[快速开始](https://aex.dev/brain/docs/quickstart) · [文档](https://aex.dev/brain/docs) ·
[官方扩展](https://github.com/aexhq/extensions) · [English](README.md)

Brain 是一个开源的 AI agent 服务器。连接模型和工具，发送消息，即可读取回答。
Brain 保存对话、工具结果和进度，方便应用之后继续使用或查看。

- 把应用中的函数交给 agent 调用，例如查询订单或搜索数据。
- 实时查看输出，事后查看会话历史。
- 使用现成的 agent loop，也可以编写自己的逻辑。
- 自行部署 Brain，或使用 [Aex](https://aex.dev) 托管。

> **早期预览。** 1.0 之前 API 可能变化。服务器重启后保留已保存的历史；中断的工作会报告失败，不会自动重试。

## 快速开始

需要 Docker、Node.js 22 或更新版本，以及 OpenAI API key。启动服务器：

```sh
docker run --rm -p 127.0.0.1:8080:8080 \
  -e BRAIN_LISTEN=0.0.0.0:8080 -e BRAIN_API_TOKEN=quickstart \
  -v brain-data:/var/lib/brain ghcr.io/aexhq/brain:latest
```

在另一个终端安装依赖，并设置环境变量 `OPENAI_API_KEY`：

```sh
npm install @aexhq/brain@0.30.0 @aexhq/agentloop-pi@7.0.1 zod@4
```

将以下内容保存为 `order.mjs`，运行 `node order.mjs`：

```js
import { Brain, brainEnv, tool } from "@aexhq/brain";
import { pi } from "@aexhq/agentloop-pi";
import { z } from "zod";

const lookupOrder = tool({
  name: "lookup_order",
  description: "Look up an order by id.",
  input: z.object({ id: z.string() }),
  run: ({ id }, ctx) => ctx.finish({ id, status: "shipped" }),
});

const brain = new Brain({ baseUrl: "http://127.0.0.1:8080", token: "quickstart" });
try {
  const session = await brain.sessions.create({
    model: { provider: "openai", name: "gpt-5-mini", apiKey: process.env.OPENAI_API_KEY },
    agentloop: pi({ env: brainEnv({ name: "brain" }) }),
    tools: [lookupOrder()],
  });
  try {
    await session.send("Look up order A-1001. Has it shipped?");
    console.log(JSON.stringify(await session.transcript(), null, 2));
    console.log("Session:", session.id);
  } finally {
    await session.end();
  }
} finally {
  await brain.close();
}
```

输出包含订单查询结果，以及订单 A-1001 已发货的回答。示例工具返回固定数据；可以替换为真实查询。
查询函数在你的 Node 进程中运行，agent 需要调用它时，请保持该进程在线。

## 下一步

- [会话](https://aex.dev/brain/docs/concepts/sessions)：继续对话、查看历史、停止任务。
- [编写工具](https://aex.dev/brain/docs/guides/write-a-tool)：连接自己的 API 或数据库。
- [编写 agent loop](https://aex.dev/brain/docs/guides/write-a-loop)：控制模型和工具的调用逻辑。
- [选择执行环境](https://aex.dev/brain/docs/concepts/environment)：使用应用进程、浏览器或沙箱。

客户端 SDK 支持 JavaScript 和 TypeScript。扩展指南提供 JavaScript、Rust 和 Python 示例及构建步骤；
其他客户端可使用 [HTTP API](https://aex.dev/brain/docs/reference/api)。

Brain 管理会话历史、事件和生命周期，让你专注于模型、工具和 agent 行为。
源码构建见 [Contributing](CONTRIBUTING.md)，实现原理见[设计记录](references/adrs/README.md)。

[MIT 许可证](LICENSE)。问题反馈：[GitHub Issues](https://github.com/aexhq/brain/issues)
或 [support@aex.dev](mailto:support@aex.dev)。
