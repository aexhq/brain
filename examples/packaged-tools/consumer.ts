import { hostEnv, type SessionTool } from "@aexhq/brain";
import { readText } from "@brain-example/files";

export const tools: SessionTool[] = [readText({ env: hostEnv({ name: "app" }) })];
