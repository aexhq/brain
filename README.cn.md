<pre align="center">
              ______ ______ _______ _______ _______
  ▄████▄     |   __ \   __ \   _   |_     _|    |  |
▄██▄██▄██▄   |   __ <      <       |_|   |_|       |
  ▀▀  ▀▀     |______/___|__|___|___|_______|__|____|
</pre>

<p align="center"><strong>极简、分布式、可扩展的 agent 运行时。</strong></p>

<p align="center">
  <a href="https://github.com/aexhq/brain/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/aexhq/brain/actions/workflows/ci.yml/badge.svg" /></a>
  <a href="https://www.npmjs.com/package/@aexhq/brain"><img alt="npm" src="https://img.shields.io/npm/v/%40aexhq%2Fbrain?label=%40aexhq%2Fbrain" /></a>
  <img alt="Rust" src="https://img.shields.io/badge/rust-1.97%2B-orange" />
</p>

<p align="center">
  <a href="https://aex.dev/brain/docs"><strong>文档</strong></a> ·
  <a href="https://aex.dev/brain/docs/reference/api">API 参考</a> ·
  <a href="https://aex.dev/brain">官网</a> ·
  <a href="https://github.com/aexhq/extensions">官方扩展</a> ·
  <a href="ROADMAP.md">路线图</a> ·
  <a href="README.md">English</a>
</p>

> [!NOTE]
> **早期预览。** 在发布 1.0.0 之前，API 和功能可能随时变更，不保证向后兼容，也不另行通知。

## 这是什么

**Brain** 是一个独立、极简、分布式、可扩展的 agent 运行时。应用通过小型公共接口组合 Agentloop、
模型、工具和环境。同一会话可以调用多个环境中的工具；应用自行提供产品策略、调度和基础设施。

- `component(urlOrBytes)` 包装已经编译好的 WebAssembly Component。Brain 接收原始 Wasm，
  不编译应用源码。
- 每个工具和 Agentloop 都放置在会话中一个有名字的环境里：`{ env, ...options }`。
- `brainEnv({ name })` 是 Brain 内置的环境，在服务器内用 Wasmtime 运行 Component；每次调用只获得
  其 `needs` 所声明的资源，并受服务器 `BRAIN_ENV_*` 允许列表约束，默认均不授权。
- `hostEnv({ name })` 是你自己的进程，向 Brain 注册为 host；带 `run` 的工具放在这里。
- `environment({ url, credential, configure })` 是通过 HTTP 访问的环境扩展，每个实例由应用配置。
- 工具用 `needs` 声明所需的 URI（`pkg:`、`https:`、`file:`），由环境提供或在 setup 时拒绝，Brain 不读取。
- 每次 Wasm 调用最多使用一百亿个 Wasmtime fuel 单位执行 guest 代码；挂起的 I/O 不消耗 fuel，
  session 的墙钟时间限制仍约束整个 turn。

## 持久化规则

每个会话只有一份规范日志。会话状态、公共事件、对话记录和 Agentloop kv 都是它的投影。
Brain 在发送外部副作用之前先把意图持久化提交，只发送一次，绝不自动重试。已知结果、已知失败或
未知结果都会在返回 Agentloop 之前提交。替换现有对话尾部的规范记录同时投影为
`transcript_replaced` 事件；纯追加不产生重复事件。

放在 host env 里的工具通过一条 host SSE 连接接收命令。`ctx.emit(kind, data)` 把扩展事件提交到同一份日志，
Promise 在提交完成后才返回。

## 架构

Agentloop 控制上下文，决定何时调用模型或工具；Brain 协调执行并记录结果。
每个环境都通过同一套协议访问：服务器内的 brain env、作为你自己进程的 host env，以及通过 HTTP 访问的任何环境。

![Brain 架构](references/architecture.png)

每次执行使用新的 Wasm Store，对话与已记录事件仍可读取。调用方决定环境的存活时间；环境只实现
setup、execute、detach 和 teardown，不负责空闲过期策略。工作目录按会话和环境名称隔离，保留到 teardown。
同一个工具可放在多个环境中，每次调用必须明确选择已授权的环境。Agentloop 可以隐藏环境选择，也可以交给模型选择。

Brain 是基于 [Tokio](https://tokio.rs/) 的原生 Rust 二进制文件，用
[Axum](https://github.com/tokio-rs/axum) 提供 HTTP 和 SSE API，本地部署无需外部存储。

## 快速开始

运行服务器：

```sh
docker run --rm -p 127.0.0.1:8080:8080 \
  -e BRAIN_LISTEN=0.0.0.0:8080 -e BRAIN_API_TOKEN=quickstart \
  -v brain-data:/var/lib/brain ghcr.io/aexhq/brain:latest
```

```sh
npm install @aexhq/brain @aexhq/agentloop-pi zod
```

```js
import { Brain, brainEnv, hostEnv, tool } from "@aexhq/brain";
import { pi } from "@aexhq/agentloop-pi";
import { z } from "zod";

const orders = { "A-1001": { status: "shipped", eta: "Thursday" } };
const lookupOrder = tool({
  name: "lookup_order",
  description: "Look up an order's status by id.",
  input: z.object({ id: z.string() }),
  run: async ({ id }, ctx) => {
    await ctx.emit("order_lookup_started", { id });
    return orders[id] ?? { status: "unknown order" };
  },
});

const brain = new Brain({ baseUrl: "http://127.0.0.1:8080", token: "quickstart" });
const session = await brain.sessions.create({
  model: { provider: "openai", name: "gpt-5-mini", apiKey: process.env.OPENAI_API_KEY },
  agentloop: pi({ env: brainEnv({ name: "brain" }) }),
  tools: [lookupOrder({ env: hostEnv({ name: "app" }) })],
});

await session.send("Where is order A-1001?");
for await (const event of session.events()) console.log(event.sequence, event.type);
await session.end();
await session.delete();
process.exit(0);
```

自定义 Agentloop 直接提供 Component：

```js
import { agentloop, brainEnv, component } from "@aexhq/brain";

const custom = agentloop({
  implementation: component(new URL("./agentloop.wasm", import.meta.url)),
});
const placed = custom({ env: brainEnv({ name: "brain" }) });
```

## 性能与生命周期

默认每轮结束后释放会话执行状态，重启时按需读取会话，不扫描所有对话日志。可重建的检查点加快恢复；
已编译并预链接的 Wasm 模板跨调用复用，每次调用使用新的实例。原生工具拥有独立于 Agentloop 的并发容量。
历史对比数据见 [BENCHMARKS.md](BENCHMARKS.md)，不能代表当前实现的性能承诺。

## 联系方式

支持与 bug 反馈请提交 [issue](https://github.com/aexhq/brain/issues) 或写信至
[support@aex.dev](mailto:support@aex.dev)。合作与商务事宜请写信至
[admin@aex.dev](mailto:admin@aex.dev)。
