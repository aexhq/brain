import { InMemoryCredentialStore, InMemoryModelsStore } from '@earendil-works/pi-ai';
import { ModelRuntime } from '@earendil-works/pi-coding-agent';

// Mirror the runner's offline catalog policy without reading user credentials,
// local model overrides, or a persisted catalog from the developer's machine.
const runtime = await ModelRuntime.create({
  credentials: new InMemoryCredentialStore(), modelsStore: new InMemoryModelsStore(),
  modelsPath: null, allowModelNetwork: false, refreshOnCreate: false,
});
const models = runtime.getModels().map(({ provider, id, api, contextWindow }) => ({ provider, id, api, contextWindow }));
models.sort((a, b) => `${a.provider}/${a.id}`.localeCompare(`${b.provider}/${b.id}`, 'en'));
process.stdout.write(JSON.stringify(models));
