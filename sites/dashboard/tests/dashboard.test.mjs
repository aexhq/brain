import assert from 'node:assert/strict';
import { before, after, test } from 'node:test';
import { createRequire } from 'node:module';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { createServer } from 'node:net';
import { setTimeout as delay } from 'node:timers/promises';
import { mkdir, readFile } from 'node:fs/promises';
import { fixture } from './fixture.mjs';

const requireBrowser = createRequire(
  new URL('../../../tools/browser/package.json', import.meta.url),
);
const { chromium } = requireBrowser('playwright');
const requireNext = createRequire(new URL('../package.json', import.meta.url));
let f,
  child,
  browser,
  origin,
  output = '';
const site = fileURLToPath(new URL('../', import.meta.url));
before(
  async () => {
    f = await fixture();
    const socket = createServer();
    socket.listen(0, '127.0.0.1');
    await once(socket, 'listening');
    const port = socket.address().port;
    await new Promise((resolve) => socket.close(resolve));
    origin = `http://127.0.0.1:${port}`;
    child = spawn(
      process.execPath,
      [requireNext.resolve('next/dist/bin/next'), 'start', '-H', '127.0.0.1', '-p', String(port)],
      {
        cwd: site,
        env: { ...process.env, BRAIN_API_URL: f.url, NEXT_TELEMETRY_DISABLED: '1' },
        windowsHide: true,
        stdio: ['ignore', 'pipe', 'pipe'],
      },
    );
    child.stdout.on('data', (data) => {
      output += data;
    });
    child.stderr.on('data', (data) => {
      output += data;
    });
    let ready = false;
    for (let i = 0; i < 200; i++) {
      if (child.exitCode !== null) throw new Error(output);
      try {
        if ((await fetch(origin)).ok) {
          ready = true;
          break;
        }
      } catch {}
      await delay(100);
    }
    assert.ok(ready, output);
    browser = await chromium.launch({ headless: true });
  },
  { timeout: 40000 },
);
after(async () => {
  await browser?.close();
  if (child && child.exitCode === null) {
    child.kill();
    await once(child, 'exit');
  }
  await f?.close();
});

async function login(token = 'test-a') {
  const response = await fetch(`${origin}/api/connection`, {
    method: 'POST',
    headers: { origin, 'content-type': 'application/json' },
    body: JSON.stringify({ token }),
  });
  assert.equal(response.status, 200);
  assert.match(response.headers.get('set-cookie'), /HttpOnly/i);
  assert.match(response.headers.get('set-cookie'), /SameSite=strict/i);
  assert.deepEqual(await response.json(), { connected: true });
  return response.headers.get('set-cookie').split(';')[0];
}
async function pageFor(t, token = 'test-a', width = 1440) {
  const context = await browser.newContext({ viewport: { width, height: 1000 } });
  t.after(() => context.close());
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', (error) => errors.push(error.message));
  t.after(() => assert.deepEqual(errors, []));
  if (width < 700) {
    await page.route('**/_next/static/**/*.js', async (route) => {
      await delay(400);
      await route.continue();
    });
  }
  await page.goto(origin, { waitUntil: 'commit' });
  await page.getByLabel('Brain API token').fill(token);
  await page.getByRole('button', { name: 'Connect to Brain' }).click();
  await page
    .getByRole('heading', { name: 'Sessions', exact: true })
    .waitFor()
    .catch(async (error) => {
      throw new Error(
        `${error.message}\n${page.url()}\n${await page.locator('body').innerText()}\n${output}`,
      );
    });
  return page;
}

test('proxy enforces cookie authentication, origin, explicit routes, queries, and account scope', async () => {
  assert.equal((await fetch(`${origin}/api/brain/sessions`)).status, 401);
  assert.equal(
    (
      await fetch(`${origin}/api/connection`, {
        method: 'POST',
        headers: { origin: 'https://evil.invalid', 'content-type': 'application/json' },
        body: JSON.stringify({ token: 'test-a' }),
      })
    ).status,
    403,
  );
  const cookie = await login();
  const attachmentURL = `${origin}/api/brain/sessions/${encodeURIComponent('support/issue-42')}/attachments`;
  const uploaded = await fetch(`${attachmentURL}?name=notes.html`, {
    method: 'POST',
    headers: { cookie, origin, 'content-type': 'text/html' },
    body: '<h1>Untrusted attachment</h1>',
  });
  assert.equal(uploaded.status, 201);
  const { reference } = await uploaded.json();
  const downloaded = await fetch(`${attachmentURL}?reference=${reference}`, {
    headers: { cookie },
  });
  assert.equal(downloaded.status, 200);
  assert.match(downloaded.headers.get('content-disposition'), /^attachment;/);
  assert.equal(downloaded.headers.get('x-content-type-options'), 'nosniff');
  assert.equal(await downloaded.text(), '<h1>Untrusted attachment</h1>');
  assert.equal(
    (
      await fetch(`${origin}/api/brain/sessions/${encodeURIComponent('support/issue-42')}`, {
        headers: { cookie },
      })
    ).status,
    200,
  );
  assert.equal((await fetch(`${origin}/api/brain/executors`, { headers: { cookie } })).status, 404);
  assert.equal(
    (await fetch(`${origin}/api/brain/sessions?tenant=b`, { headers: { cookie } })).status,
    404,
  );
  assert.equal(
    (await fetch(`${origin}/api/brain/sessions?limit=2&limit=3`, { headers: { cookie } })).status,
    404,
  );
  assert.equal(
    (
      await fetch(`${origin}/api/brain/sessions`, {
        method: 'POST',
        headers: { cookie, origin: 'https://evil.invalid' },
        body: '{}',
      })
    ).status,
    403,
  );
  const other = await login('test-b');
  assert.equal(
    (
      await fetch(`${origin}/api/brain/sessions/${encodeURIComponent('support/issue-42')}`, {
        headers: { cookie: other },
      })
    ).status,
    404,
  );
  assert.deepEqual(
    (await (await fetch(`${origin}/api/brain/sessions`, { headers: { cookie: other } })).json())
      .sessions,
    [],
  );
  const reader = await login('test-read');
  assert.equal(
    (
      await fetch(`${origin}/api/brain/sessions`, {
        method: 'POST',
        headers: { cookie: reader, origin, 'content-type': 'application/json' },
        body: '{}',
      })
    ).status,
    403,
  );
  assert.equal(
    (
      await fetch(`${origin}/api/brain/sessions`, {
        method: 'POST',
        headers: { cookie, origin, 'content-type': 'application/json' },
        body: ' '.repeat(81 * 1024),
      })
    ).status,
    413,
  );
});

test('real browser browses sessions, tools, recorded activity and usage without exposing its token', async (t) => {
  const page = await pageFor(t);
  await page.locator('a.session-link').filter({ hasText: 'support/issue-42' }).waitFor();
  assert.equal(await page.evaluate(() => localStorage.length), 0);
  assert.ok(!(await page.content()).includes('test-a'));
  assert.ok(!(await page.evaluate(() => document.cookie)).includes('brain_dashboard'));
  await page.getByLabel('Filter loaded sessions').fill('document');
  assert.equal(await page.locator('a.session-link').count(), 1);
  await page.getByLabel('Filter loaded sessions').fill('');
  await mkdir(new URL('../artifacts/', import.meta.url), { recursive: true });
  await page.screenshot({
    path: fileURLToPath(new URL('../artifacts/sessions-desktop.png', import.meta.url)),
    fullPage: true,
  });
  await page.setViewportSize({ width: 390, height: 844 });
  assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
  await page.screenshot({
    path: fileURLToPath(new URL('../artifacts/sessions-populated-mobile.png', import.meta.url)),
    fullPage: true,
  });
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.getByRole('link', { name: 'Tools', exact: true }).click();
  await page.getByRole('heading', { name: 'lookup_release' }).waitFor();
  await page.screenshot({
    path: fileURLToPath(new URL('../artifacts/tools-desktop.png', import.meta.url)),
    fullPage: true,
  });
  await page.getByText('Input schema').click();
  assert.ok((await page.locator('pre').innerText()).includes('version'));
  await page.getByRole('link', { name: 'Sessions', exact: true }).click();
  await page.getByText('Open by ID', { exact: true }).click();
  await page.getByLabel('Session ID to open').fill('support/issue-42');
  await page.getByRole('button', { name: 'Open', exact: true }).click();
  await page
    .getByRole('heading', { name: 'support/issue-42' })
    .waitFor({ timeout: 5000 })
    .catch(async (error) => {
      throw new Error(
        error.message +
          '\n' +
          page.url() +
          '\n' +
          (await page.locator('body').innerText()) +
          '\n' +
          output,
      );
    });
  await page.getByText('The latest release adds session inspection', { exact: false }).waitFor();
  await page.locator('.tool-call summary').first().click();
  await page.getByText('The release adds session inspection', { exact: false }).waitFor();
  await page.getByLabel('Inspect assistant message').click();
  await page.getByRole('dialog').waitFor();
  await page.keyboard.press('Escape');
  await page.getByRole('tab', { name: 'Usage' }).click();
  await page.getByRole('button', { name: 'Load full history' }).click();
  await page.getByText('1,240', { exact: true }).waitFor();
  await page.getByText('Full history ·', { exact: false }).waitFor();
  await page.getByRole('tab', { name: 'Configuration' }).click();
  await page.getByRole('heading', { name: 'Instructions', exact: true }).waitFor();
  await page.getByText('View JSON', { exact: true }).click();
  assert.ok(
    (await page.locator('.configuration-tab pre').innerText()).includes('openai/gpt-4.1-mini'),
  );
  await page.getByRole('tab', { name: 'Conversation' }).click();
  await page.screenshot({
    path: fileURLToPath(new URL('../artifacts/session-desktop.png', import.meta.url)),
    fullPage: true,
  });
  await page.setViewportSize({ width: 390, height: 844 });
  assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
  assert.ok(await page.getByLabel('Message', { exact: true }).isVisible());
  await page.screenshot({
    path: fileURLToPath(new URL('../artifacts/session-mobile.png', import.meta.url)),
    fullPage: true,
  });
});

test('create, attach, send, reconnect, resolve uncertain acceptance, resume, close and delete', async (t) => {
  const page = await pageFor(t);
  await page.getByRole('link', { name: 'New session' }).click();
  await page.getByRole('heading', { name: 'New session', exact: true }).waitFor();
  await page.getByLabel('Model API key', { exact: false }).fill('private-model-test');
  await page.getByLabel('Instructions', { exact: false }).fill('Answer accurately and concisely.');
  await page.getByRole('checkbox', { name: 'lookup_release', exact: false }).check();
  await page.screenshot({
    path: fileURLToPath(new URL('../artifacts/new-session-desktop.png', import.meta.url)),
    fullPage: true,
  });
  await page.getByText('Advanced options', { exact: true }).click();
  await page.getByLabel('Session ID', { exact: false }).fill('dashboard%2Ftest/✓');
  await page.getByText('TypeScript example', { exact: true }).click();
  assert.ok(!(await page.locator('.code-preview pre').innerText()).includes('private-model-test'));
  await page.getByRole('button', { name: 'Create session', exact: true }).click();
  await page
    .getByRole('heading', { name: 'dashboard%2Ftest/✓' })
    .waitFor({ timeout: 5000 })
    .catch(async (error) => {
      throw new Error(
        error.message +
          '\n' +
          page.url() +
          '\n' +
          (await page.locator('body').innerText()) +
          '\n' +
          output,
      );
    });
  await page.getByLabel('Message', { exact: true }).waitFor();
  await page.locator('input[type=file]').setInputFiles({
    name: 'notes.txt',
    mimeType: 'text/plain',
    buffer: Buffer.from('Release notes'),
  });
  await page.getByText('notes.txt', { exact: true }).waitFor();
  await page.getByLabel('Message', { exact: true }).fill('Summarize release');
  await page.getByRole('button', { name: 'Send message', exact: true }).click();
  await page.getByText('Finished: Summarize release', { exact: true }).waitFor();
  assert.equal(
    f.requests.find((request) => request.body?.text === 'Summarize release').body.attachments
      .length,
    1,
  );
  const downloadEvent = page.waitForEvent('download');
  await page.getByRole('link', { name: 'Download attachment', exact: false }).click();
  const download = await downloadEvent;
  assert.equal(download.suggestedFilename(), 'notes.txt');
  assert.equal(await readFile(await download.path(), 'utf8'), 'Release notes');
  f.disconnectStreams('dashboard%2Ftest/✓');
  f.publish(
    'dashboard%2Ftest/✓',
    'assistant.message',
    { text: 'Recovered after disconnection.' },
    'recovered',
  );
  await page.getByText('Recovered after disconnection.', { exact: true }).waitFor();
  assert.equal(await page.getByText('Recovered after disconnection.', { exact: true }).count(), 1);
  const count = f.executions;
  await page.getByLabel('Message', { exact: true }).fill('Lose acknowledgement');
  await page.getByRole('button', { name: 'Send message', exact: true }).click();
  await page.getByRole('button', { name: 'Retry same message' }).waitFor();
  await page.getByRole('button', { name: 'Retry same message' }).click();
  await page.getByText('Finished: Lose acknowledgement', { exact: true }).waitFor();
  assert.equal(f.executions, count + 1);
  f.status('dashboard%2Ftest/✓', 'running');
  await page.getByRole('button', { name: 'Interrupt', exact: true }).click();
  await page.getByRole('button', { name: 'Resume', exact: true }).click();
  await page.getByRole('button', { name: 'Send message', exact: true }).waitFor();
  await page.getByLabel('Session actions').click();
  await page.getByRole('button', { name: 'Close session', exact: true }).click();
  await page
    .getByRole('dialog')
    .getByRole('button', { name: 'Close session', exact: true })
    .click();
  await page.getByText('This session is closed.', { exact: false }).waitFor();
  f.pendingCleanup();
  await page.getByLabel('Session actions').click();
  await page.getByRole('button', { name: 'Delete session', exact: true }).click();
  await page
    .getByRole('dialog')
    .getByRole('button', { name: 'Delete session', exact: true })
    .click();
  await page
    .getByRole('dialog')
    .getByText('Resource cleanup is pending.', { exact: false })
    .waitFor();
  await page
    .getByRole('dialog')
    .getByRole('button', { name: 'Delete session', exact: true })
    .click();
  await page.getByRole('heading', { name: 'Sessions', exact: true }).waitFor();
  assert.ok(!f.sessions.has('dashboard%2Ftest/✓'));
});

test('mobile navigation, empty account, and sign out are usable', async (t) => {
  const page = await pageFor(t, 'test-b', 390);
  await page.getByRole('heading', { name: 'No sessions yet' }).waitFor();
  assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
  await page.screenshot({
    path: fileURLToPath(new URL('../artifacts/sessions-mobile.png', import.meta.url)),
    fullPage: true,
  });
  await page.getByRole('button', { name: 'Open navigation' }).click();
  await page.getByRole('link', { name: 'Connection', exact: true }).click();
  await page.getByRole('button', { name: 'Disconnect', exact: true }).click();
  await page.getByLabel('Brain API token').waitFor();
  assert.ok(!(await page.context().cookies()).some((cookie) => cookie.name === 'brain_dashboard'));
  await page.screenshot({
    path: fileURLToPath(new URL('../artifacts/connect-mobile.png', import.meta.url)),
    fullPage: true,
  });
});
