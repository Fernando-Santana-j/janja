import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, readFile, writeFile, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { httpsOrigin, options, loadKey, freeIce } from '../scripts/cloudflare.mjs';

test('origin must be HTTPS and cannot carry credentials, paths or query strings', () => {
  assert.equal(httpsOrigin('https://sala.example.org/'), 'https://sala.example.org');
  for (const value of ['http://example.org', 'https://u:p@example.org', 'https://example.org/path', 'https://example.org?q=1', 'https://example.org/#x']) assert.throws(() => httpsOrigin(value));
});
test('terminal accepts only known options and free profile has no paid relay', () => {
  assert.deepEqual(options(['--origin', 'https://sala.example.org', '--check']), { check: true, origin: 'https://sala.example.org' });
  assert.throws(() => options(['--origin']));
  assert.throws(() => options(['--turn']));
  assert.deepEqual(freeIce, [{ urls: 'stun:stun.cloudflare.com:3478' }]);
});
test('room key survives restarts and corrupt key is never replaced silently', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'sala-key-test-'));
  try {
    const first = await loadKey(directory), second = await loadKey(directory);
    assert.match(first.key, /^[a-f0-9]{64}$/);
    assert.equal(first.key, second.key);
    await writeFile(first.path, 'corrupt');
    await assert.rejects(loadKey(directory), /preservado/);
    assert.equal(await readFile(first.path, 'utf8'), 'corrupt');
  } finally { await rm(directory, { recursive: true, force: true }); }
});
