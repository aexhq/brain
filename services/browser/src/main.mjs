import { browserService } from "./browser.mjs";
const origins = JSON.parse(process.env.BROWSER_ORIGINS ?? "[]");
const service = await browserService({ token: process.env.TOOL_API_KEY, origins });
service.server.listen(Number(process.env.PORT ?? 3005), "0.0.0.0");
for (const signal of ["SIGINT", "SIGTERM"]) process.once(signal, () => { void service.close().then(() => process.exit(0), () => process.exit(1)); });
