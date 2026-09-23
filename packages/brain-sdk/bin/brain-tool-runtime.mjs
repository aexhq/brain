#!/usr/bin/env node
import { runToolProcess } from "../dist/process.js";
await runToolProcess(process.argv[2]);
await new Promise(resolve => process.stdout.write("", resolve));
process.exit(0);
