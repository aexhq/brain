import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';
import { Brain } from '../../sdks/typescript/dist/index.js';

const root = fileURLToPath(new URL('../../', import.meta.url));
const compose = path.join(root, 'deploy/docker/compose.yaml');
const temporary = path.join(root, 'bin/distributed-smoke');
await mkdir(temporary, { recursive: true });
const environment = Object.fromEntries((await readFile(path.join(root, 'deploy/docker/.env'), 'utf8')).split(/\r?\n/).filter(Boolean).map(line => { const i = line.indexOf('='); return [line.slice(0, i), line.slice(i + 1)]; }));
function docker(args) {
  const result = spawnSync('docker', args, { cwd: root, encoding: 'utf8', windowsHide: true, timeout: 120000 });
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stderr);
  return result.stdout.trim();
}
const fixtureName = 'brain-smoke-model-' + randomUUID().slice(0, 8);
const config = JSON.parse(await readFile(path.join(root, 'deploy/docker/brain.json'), 'utf8'));
config.providers.openai = { base_url: `http://${fixtureName}:8089/v1`, api: 'openai-responses' };
config.media_origins = [`http://${fixtureName}:8089`];
const configFile = path.join(temporary, 'brain.json');
const override = path.join(temporary, 'compose.json');
await writeFile(configFile, JSON.stringify(config));
const toolsConfig = JSON.parse(await readFile(path.join(root,'deploy/docker/tools.json'),'utf8'));
toolsConfig.tools=[{name:'remote_echo',description:'Remote echo',input_schema:{type:'object'},url:`http://${fixtureName}:8089/echo`,tenants:['local']}];
const toolsFile=path.join(temporary,'tools.json');await writeFile(toolsFile,JSON.stringify(toolsConfig));
await writeFile(override, JSON.stringify({ services: { brain: { volumes: [{ type: 'bind', source: configFile, target: '/config/brain.json', read_only: true }] }, tools:{volumes:[{type:'bind',source:toolsFile,target:'/config/tools.json',read_only:true}]} } }));
const sessions = [];
let fixtureStarted = false;
let failure;
const sdk = new Brain('http://127.0.0.1:8080',environment.BRAIN_API_KEY);
async function request(method, resource, body) {
  const response = await fetch('http://127.0.0.1:8080' + resource, { method, headers: { Authorization: `Bearer ${environment.BRAIN_API_KEY}`, 'Content-Type': 'application/json' }, ...(body === undefined ? {} : { body: JSON.stringify(body) }), signal: AbortSignal.timeout(60000) });
  const value = await response.json();
  assert.ok(response.ok, `${method} ${resource}: ${JSON.stringify(value)}`);
  return value;
}
const endpoint = id => '/sessions/' + encodeURIComponent(id);
async function settled(id) {
  const until = Date.now() + 60000;
  while (Date.now() < until) {
    const value = await request('GET', endpoint(id));
    if (value.status === 'idle') return value;
    if (['failed', 'interrupted'].includes(value.status)) assert.fail(JSON.stringify(await request('GET', endpoint(id) + '/events')));
    await delay(100);
  }
  assert.fail('Pi did not settle');
}
try {
  docker(['run', '--detach', '--rm', '--name', fixtureName, '--network', 'brain-runtime', '--mount', `type=bind,source=${path.join(root, 'tests')},target=/tests,readonly`, '--entrypoint', 'node', 'brain-tool-runtime:local-dev', '/tests/distributed/model.mjs']);
  fixtureStarted = true;
  docker(['compose', '-f', compose, '-f', override, 'up', '-d', '--wait', 'brain']);
  const id = 'distributed/' + randomUUID();
  await request('POST', '/sessions', { id, harness: 'pi', config: { model: 'openai/gpt-4.1-mini', tools: [] }, model_credential: { api_key: 'local-fixture-key' } });
  sessions.push(id);
  const attachment=await sdk.uploadAttachment(id,'notes.txt',new Blob(['streamed attachment content'],{type:'text/plain'}));
  assert.equal(await (await sdk.downloadAttachment(id,attachment.reference)).text(),'streamed attachment content');
  await request('POST', endpoint(id) + '/events', { type: 'user.message', text: 'first isolated turn',attachments:[attachment.reference],references:[{uri:`http://${fixtureName}:8089/reference`,media_type:'text/plain'}] });
  await settled(id);
  let events = await request('GET', endpoint(id) + '/events');
  assert.ok(events.events.some(e => e.type === 'assistant.message'));
  assert.ok(events.events.some(e=>e.type==='assistant.message'&&e.data.text.includes('streamed attachment content')&&e.data.text.includes('authorized external reference')));
  assert.deepEqual(events.events.find(e=>e.type==='user.message').data.attachments,[attachment.reference]);
  const flushed = await request('POST', endpoint(id) + '/flush');
  assert.match(flushed.checkpoint, /^[a-f0-9]{64}$/);
  docker(['compose', '-f', compose, '-f', override, 'restart', 'brain']);
  await delay(1000);
  events = await request('GET', endpoint(id) + '/events');
  assert.ok(events.events.some(e => e.type === 'assistant.message'));
  await request('POST', endpoint(id) + '/events', { type: 'user.message', text: 'second restored turn' });
  await settled(id);
  await request('POST',endpoint(id)+'/restore',{checkpoint:flushed.checkpoint});
  assert.equal((await request('GET',endpoint(id)+'/events')).events.filter(e=>e.type==='assistant.message').length,1);
  const uploaded = await sdk.uploadTool({ name: 'echo', description: 'Echo in an isolated environment', input_schema: { type: 'object' } }, `import fs from 'node:fs';import os from 'node:os';export default async request => {if(process.getuid()!==1000||fs.existsSync('/var/run/docker.sock')||fs.existsSync('/config/brain.json')||Object.keys(process.env).some(k=>k.startsWith('BRAIN_'))||Object.keys(os.networkInterfaces()).some(k=>k!=='lo'))throw Error('isolation failed');return {content:[{type:'text',text:request.arguments.value}]};};`);
  const toolSession = 'hosted/' + randomUUID();
  await request('POST', '/sessions', { id: toolSession, harness: 'pi', config: { model: 'openai/gpt-4.1-mini', tools: [], bindings: [{ alias: 'sample', provider: 'hosted', operations: ['echo'], parameters: { package: uploaded.reference } }] }, model_credential: { api_key: 'local-fixture-key' } });
  sessions.push(toolSession);
  await request('POST', endpoint(toolSession) + '/events', { type: 'user.message', text: 'CALL sample_echo {"value":"isolated hosted tool"}' });
  await settled(toolSession);
  events = await request('GET', endpoint(toolSession) + '/events');
  const result = events.events.find(e => e.type === 'tool.result');
  assert.ok(result && !result.data.result.is_error, JSON.stringify(events));
  assert.equal(result.data.result.content[0].text, 'isolated hosted tool');
  const remoteId='remote/'+randomUUID();sessions.push(remoteId);
  await sdk.create({id:remoteId,harness:'pi',config:{model:'openai/gpt-4.1-mini',tools:['remote_echo']},model_credential:{api_key:'local-fixture-key'}});
  await sdk.send(remoteId,{type:'user.message',text:'CALL remote_echo {"value":"remote tool"}'});await settled(remoteId);
  assert.equal((await sdk.history(remoteId)).events.find(e=>e.type==='tool.result').data.result.content[0].text,'remote tool');
  const browserId='browser/'+randomUUID();sessions.push(browserId);
  await sdk.create({id:browserId,harness:'pi',config:{model:'openai/gpt-4.1-mini',tools:[]},model_credential:{api_key:'local-fixture-key'},browser_tools:[{name:'echo',description:'Client execution',input_schema:{type:'object'},run:args=>args.value}]});
  await sdk.send(browserId,{type:'user.message',text:'CALL runtime_echo {"value":"browser tool"}'});await settled(browserId);
  assert.equal((await sdk.history(browserId)).events.find(e=>e.type==='tool.result').data.result.structured_content,'browser tool');
  const tail=await request('GET',endpoint(browserId)+'/events?tail=true&limit=2');assert.equal(tail.events.length,2);
  const cli=spawnSync(process.execPath,[path.join(root,'sdks/typescript/dist/cli.mjs'),'get',id],{encoding:'utf8',windowsHide:true,env:{...process.env,BRAIN_API_KEY:environment.BRAIN_API_KEY,BRAIN_URL:'http://127.0.0.1:8080'}});
  assert.equal(cli.status,0,cli.stderr);assert.equal(JSON.parse(cli.stdout).id,id);
  const started = performance.now();
  await Promise.all(Array.from({length:3}, async (_, n) => {
    const concurrentId='concurrent/'+randomUUID();sessions.push(concurrentId);
    await sdk.create({id:concurrentId,harness:'pi',config:{model:'openai/gpt-4.1-mini',tools:[]},model_credential:{api_key:'local-fixture-key'}});
    await sdk.send(concurrentId,{type:'user.message',text:`isolated parallel turn ${n}`});await settled(concurrentId);
    const reply=(await sdk.history(concurrentId)).events.find(e=>e.type==='assistant.message');
    assert.equal(reply.data.text,`answer: isolated parallel turn ${n}`);
  }));
  console.log(`Three concurrent isolated sessions completed in ${((performance.now()-started)/1000).toFixed(2)}s (local fixture, not a capacity claim).`);
  const cancelled='cancel/'+randomUUID();sessions.push(cancelled);
  await sdk.create({id:cancelled,harness:'pi',config:{model:'openai/gpt-4.1-mini',tools:[]},model_credential:{api_key:'local-fixture-key'}});
  await sdk.send(cancelled,{type:'user.message',text:'BLOCK until cancelled'});
  let running=false;
  for(let n=0;n<300;n++){if((await sdk.get(cancelled)).status==='running'){running=true;break;}await delay(100);}
  assert.ok(running,'cancellation fixture never started');
  await sdk.interrupt(cancelled);
  let interrupted=false;
  for(let n=0;n<300;n++){if((await sdk.get(cancelled)).status==='interrupted'){interrupted=true;break;}await delay(100);}
  assert.ok(interrupted,'interruption did not settle');
  const telemetryUntil=Date.now()+25000;
  let observed=false;
  while(Date.now()<telemetryUntil){
    const logs=docker(['compose','-f',compose,'logs','--since','2m','telemetry']);
    if(logs.includes('"otelcol.signal": "traces"')&&logs.includes('"otelcol.signal": "metrics"')){observed=true;break;}
    await delay(1000);
  }
  assert.ok(observed,'collector did not receive both traces and metrics');
  console.log('Distributed Pi, attachments/references, recovery, all three tool flavours, isolation, cancellation, telemetry, CLI and external flush passed.');
} catch (error) {
  console.error(docker(['compose', '-f', compose, 'logs', '--tail', '30', 'brain', 'sandbox', 'tools']));
  failure = error;
} finally {
  const cleanup=[];
  try { await sdk.dispose(); } catch(error) { cleanup.push(error); }
  for (const id of sessions) {
    try { await request('DELETE', endpoint(id)); } catch (error) { cleanup.push(error); }
  }
  try { docker(['compose', '-f', compose, 'up', '-d', '--wait', 'tools','brain']); } catch(error) { cleanup.push(error); }
  if(fixtureStarted) { try { docker(['rm', '--force', fixtureName]); } catch(error) { cleanup.push(error); } }
  if(failure || cleanup.length) throw new AggregateError([...(failure ? [failure] : []),...cleanup], 'Distributed verification or cleanup failed');
}
