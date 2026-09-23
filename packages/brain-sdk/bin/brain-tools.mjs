#!/usr/bin/env node
import { packageTools } from "../dist/package.js";
await packageTools(process.argv[2]);
