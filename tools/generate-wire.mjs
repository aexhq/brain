import { mkdir, readFile, writeFile } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const schemaText = await readFile(path.join(root, "crates/brain-protocol/generated/contract/session/v2/schemas.json"), "utf8");
const schema = JSON.parse(schemaText);
const document = JSON.parse(await readFile(path.join(root, "crates/brain-http/generated/contract/session/v2/openapi.json"), "utf8"));
const catalog = await readFile(path.join(root, "crates/brain-protocol/generated/contract/session/v2/catalog.json"), "utf8");
const definitions = schema.$defs;
const nameOf = ref => ref.split("/").at(-1);
const pascal = name => name.split(/[^a-zA-Z0-9]/u).map(part => part[0]?.toUpperCase() + part.slice(1)).join("");
const goName = name => ({ id: "ID", session_id: "SessionID", run_id: "RunID", operation_id: "OperationID", command_id: "CommandID", request_id: "RequestID" })[name] ?? pascal(name).replace(/Id$/u, "ID");
const go = ["// Code generated from brain-protocol Rust types. DO NOT EDIT.", "package wire", 'import ("encoding/json"; _ "embed")', "//go:embed schema.json", "var Schema []byte", "//go:embed catalog.json", "var Catalog []byte", ""];
const python = [
    '"""Generated from brain-protocol Rust types. Do not edit."""',
    "from __future__ import annotations",
    "from typing import Any, Literal, NotRequired, TypeAlias, TypedDict",
    "",
];
const aliases = [];

function typeGo(node) {
    if (node.$ref) return nameOf(node.$ref);
    if (node.type === "string" || typeof node.const === "string") return "string";
    if (node.type === "boolean") return "bool";
    if (node.type === "integer") return node.minimum >= 0 ? "uint64" : "int64";
    if (node.type === "number") return "float64";
    if (node.type === "array") return `[]${typeGo(node.items)}`;
    if (node.type === "object" && !node.properties && node.additionalProperties && typeof node.additionalProperties === "object") return `map[string]${typeGo(node.additionalProperties)}`;
    return "json.RawMessage";
}

function typePython(node) {
    if (node.$ref) return nameOf(node.$ref);
    if (node.const !== undefined) return `Literal[${JSON.stringify(node.const)}]`;
    if (node.enum) return `Literal[${node.enum.map(value => JSON.stringify(value)).join(", ")}]`;
    if (node.type === "string") return "str";
    if (node.type === "boolean") return "bool";
    if (node.type === "integer") return "int";
    if (node.type === "number") return "float";
    if (node.type === "array") return `list[${typePython(node.items)}]`;
    if (node.type === "object" && !node.properties && typeof node.additionalProperties === "object") return `dict[str, ${typePython(node.additionalProperties)}]`;
    return "Any";
}

function objectGo(name, node) {
    go.push(`type ${name} struct {`);
    for (const [key, value] of Object.entries(node.properties ?? {})) {
        let type = typeGo(value);
        const required = node.required?.includes(key);
        if (!required && !Object.hasOwn(value, "default") && !type.startsWith("[]") && !type.startsWith("map[") && type !== "json.RawMessage") type = `*${type}`;
        go.push(`${goName(key)} ${type} \`json:"${key}${required ? "" : ",omitempty"}"\``);
    }
    go.push("}", "");
}

function objectPython(name, node) {
    python.push(`class ${name}(TypedDict):`);
    if (!Object.keys(node.properties ?? {}).length) python.push("    pass");
    for (const [key, value] of Object.entries(node.properties ?? {})) {
        const type = typePython(value);
        python.push(`    ${key}: ${node.required?.includes(key) ? type : `NotRequired[${type}]`}`);
    }
    python.push("");
}

for (const [name, node] of Object.entries(definitions).sort(([a], [b]) => a.localeCompare(b, "en"))) {
    if (node.enum) {
        go.push(`type ${name} string`, "const (");
        for (const value of node.enum) go.push(`${name}${pascal(value)} ${name} = ${JSON.stringify(value)}`);
        go.push(")", "");
        aliases.push(`${name}: TypeAlias = ${typePython(node)}`);
    } else if (node.oneOf || node.anyOf) {
        const variants = node.oneOf ?? node.anyOf;
        if (!variants.every(value => value.type === "object" && value.properties?.kind?.const)) throw new Error(`unsupported union ${name}`);
        const combined = { properties: {}, required: variants[0].required.filter(key => variants.every(value => value.required.includes(key))) };
        const names = [];
        for (const variant of variants) {
            const variantName = `${name}${pascal(variant.properties.kind.const)}`;
            objectGo(variantName, variant);
            objectPython(variantName, variant);
            names.push(variantName);
            for (const [key, value] of Object.entries(variant.properties)) {
                if (key !== "kind" && combined.properties[key] && JSON.stringify(combined.properties[key]) !== JSON.stringify(value)) throw new Error(`ambiguous union field ${name}.${key}`);
                combined.properties[key] = key === "kind" ? { type: "string" } : value;
            }
        }
        objectGo(name, combined);
        aliases.push(`${name}: TypeAlias = ${names.join(" | ")}`);
    } else if (node.type === "object" && node.properties) {
        objectGo(name, node);
        objectPython(name, node);
    } else {
        go.push(`type ${name} ${typeGo(node)}`, "");
        aliases.push(`${name}: TypeAlias = ${typePython(node)}`);
    }
}

const routes = {};
for (const [route, methods] of Object.entries(document.paths)) {
    for (const [method, operation] of Object.entries(methods)) {
        if (!["get", "post", "put", "delete", "patch"].includes(method)) continue;
        const request = operation.requestBody?.content?.["application/json"]?.schema?.$ref;
        routes[`${method.toUpperCase()} ${route}`] = request ? nameOf(request) : "";
    }
}
go.push("var Routes = map[string]string{", ...Object.entries(routes).map(([route, request]) => `${JSON.stringify(route)}: ${JSON.stringify(request)},`), "}", "");
const out = path.join(root, "internal/generated/wire");
await mkdir(out, { recursive: true });
await writeFile(path.join(out, "types.go"), go.join("\n"));
await writeFile(path.join(out, "schema.json"), schemaText);
await writeFile(path.join(out, "catalog.json"), catalog);
execFileSync("gofmt", ["-w", path.join(out, "types.go")]);
const py = path.join(root, "packages/brain-python/brain/generated");
await mkdir(py, { recursive: true });
await writeFile(path.join(py, "__init__.py"), `${python.join("\n")}\n${aliases.join("\n")}\n`);
await writeFile(path.join(py, "schema.json"), schemaText);
await writeFile(path.join(py, "routes.json"), `${JSON.stringify(routes, null, 2)}\n`);
console.log("Generated Go and Python wire bindings from canonical Rust contracts.");
