# Package a tool

This library exports `readText`, a tool that reads a UTF-8 file. Start with the
[tool guide](https://aex.dev/brain/docs/guides/write-a-tool) and Node.js 22 or newer.
From this directory:

```sh
npm install
npm run build
npm pack
```

`runtime.ts` contains the implementation. The build generates a client factory that can be
imported without loading Node's filesystem APIs. `readText()` runs in the registering application,
using its working directory and permissions. `readText({ env })` runs in a provider that supports
the package; that provider must be able to install the published version.

## Check a fresh consumer

Create a separate project outside the Brain checkout. Install the tarball printed by `npm pack`
and the matching SDK version:

```sh
npm init -y
npm pkg set type=module
npm install /path/to/brain-example-files-1.0.0.tgz @aexhq/brain@0.37.0
npm install --save-dev typescript@5.9.2 @types/node@22
```

Copy [consumer.ts](consumer.ts) into that project and run:

```sh
npx tsc --noEmit --strict --module NodeNext --target ES2023 consumer.ts
```

Compilation must succeed with both the tool factory and the consumer's environment. This catches
SDK-version mismatches that building the package alone cannot catch. No Brain server or model key
is needed for this check.
