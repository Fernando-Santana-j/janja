import { spawn, spawnSync } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { mkdir, readFile, writeFile, mkdtemp, rm, access } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { createServer } from 'node:net';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
export const freeIce = [{ urls: 'stun:stun.cloudflare.com:3478' }];

export function httpsOrigin(value) {
  const url = new URL(value);
  if (url.protocol !== 'https:' || url.username || url.password || url.pathname !== '/' || url.search || url.hash) {
    throw new Error('Informe apenas a origem HTTPS, por exemplo https://sala.seudominio.com.');
  }
  return url.origin;
}

export function options(args) {
  const result = { check: false, origin: null };
  while (args.length) {
    const arg = args.shift();
    if (arg === '--check') result.check = true;
    else if (arg === '--origin' && args.length && !result.origin) result.origin = httpsOrigin(args.shift());
    else throw new Error('Uso: node scripts/cloudflare.mjs [--origin https://sala.seudominio.com] [--check]');
  }
  return result;
}

export async function loadKey(directory) {
  await mkdir(directory, { recursive: true, mode: 0o700 });
  const path = join(directory, 'room-key.txt');
  try { await writeFile(path, randomBytes(32).toString('hex'), { flag: 'wx', mode: 0o600 }); }
  catch (error) { if (error.code !== 'EEXIST') throw error; }
  const key = (await readFile(path, 'utf8')).trim();
  if (!/^[a-f0-9]{64}$/.test(key)) throw new Error(`Chave inválida em ${path}; arquivo preservado.`);
  return { key, path };
}

async function main() {
  const opt = options(process.argv.slice(2));
  const binary = process.env.SALA_BINARY || join(root, 'target', 'release', process.platform === 'win32' ? 'sala.exe' : 'sala');
  const cloudflared = process.env.CLOUDFLARED || 'cloudflared';
  let binaryExists = true;
  try { await access(binary); } catch { binaryExists = false; }
  const probe = spawnSync(cloudflared, ['--version'], { stdio: 'ignore' });
  const tunnelExists = !probe.error && probe.status === 0;
  if (opt.check) {
    console.log(`Executável Rust: ${binaryExists ? 'OK' : 'ausente (cargo build --release --locked)'}`);
    console.log(`cloudflared: ${tunnelExists ? 'OK' : probe.error?.code === 'EPERM' ? 'execução bloqueada pelo ambiente' : 'ausente ou não executável'}`);
    console.log(`Modo: ${opt.origin ? 'hostname existente; configure a rota para http://127.0.0.1:8787' : 'Quick Tunnel temporário'}`);
    console.log('Mídia: STUN gratuito, sem TURN e sem ativação de cobrança.');
    process.exitCode = binaryExists && (opt.origin || tunnelExists) ? 0 : 1;
    return;
  }
  if (!binaryExists) throw new Error('Compile primeiro: cargo build --release --locked (ou defina SALA_BINARY).');
  if (!opt.origin && !tunnelExists) throw new Error('Instale cloudflared no PC servidor. Windows: winget install --id Cloudflare.cloudflared --exact');
  await new Promise((accept, reject) => {
    const probe = createServer();
    probe.once('error', error => reject(new Error(`Não foi possível reservar localhost:8787 (${error.code}). Feche outra instância ou verifique as permissões.`)));
    probe.listen(8787, '127.0.0.1', () => probe.close(accept));
  });
  const directory = join(root, '.sala-cloudflare');
  const { key, path } = await loadKey(directory);
  let temp, tunnel, server, stopping = false;
  function stop() {
    if (stopping) return;
    stopping = true;
    server?.kill(); tunnel?.kill();
  }
  process.once('SIGINT', stop); process.once('SIGTERM', stop);
  try {
    let origin = opt.origin;
    if (!origin) {
      // Dedicated config prevents Quick Tunnel from reading the user's existing tunnel configuration.
      temp = await mkdtemp(join(tmpdir(), 'sala-cloudflare-'));
      const config = join(temp, 'config.yml');
      await writeFile(config, 'loglevel: info\n', { mode: 0o600 });
      origin = await new Promise((accept, reject) => {
        tunnel = spawn(cloudflared, ['tunnel', '--config', config, '--no-autoupdate', '--url', 'http://127.0.0.1:8787'], { stdio: ['ignore', 'pipe', 'pipe'] });
        const timer = setTimeout(() => { tunnel.kill(); reject(new Error('Cloudflare não retornou uma URL em 60 segundos. Confira a conexão.')); }, 60_000);
        let tail = '';
        const output = data => {
          tail = (tail + data.toString()).slice(-8192);
          const match = tail.match(/https:\/\/[a-z0-9-]+\.trycloudflare\.com\b/);
          if (match) { clearTimeout(timer); accept(httpsOrigin(match[0])); }
        };
        tunnel.stdout.on('data', output); tunnel.stderr.on('data', output);
        tunnel.once('error', error => { clearTimeout(timer); reject(error); });
        tunnel.once('exit', () => { clearTimeout(timer); reject(new Error('cloudflared encerrou antes de disponibilizar a sala.')); if (server && !stopping) { console.error('O túnel encerrou. Reinicie o comando.'); process.exitCode = 1; stop(); } });
      });
    }
    if (stopping || (tunnel && tunnel.exitCode !== null)) throw new Error('Inicialização interrompida.');
    server = spawn(binary, ['serve'], {
      stdio: ['inherit', 'pipe', 'inherit'],
      env: { ...process.env, SALA_KEY: key, SALA_ORIGIN: origin, SALA_PORT: '8787', SALA_ICE_SERVERS: JSON.stringify(freeIce) },
    });
    await new Promise((accept, reject) => {
      let output = '';
      server.stdout.on('data', data => { output = (output + data.toString()).slice(-1024); if (output.includes('Sala em http://')) accept(); });
      server.once('error', reject);
      server.once('exit', () => reject(new Error('Servidor encerrou antes de iniciar; confira se a porta 8787 está ocupada.')));
    });
    const roomFile = join(directory, 'room.txt');
    await writeFile(roomFile, `${origin}\n`, { mode: 0o600 });
    console.log(`Sala: ${origin}\nChave salva em: ${path}\nURL para os amigos: ${roomFile}`);
    console.log(opt.origin ? 'O túnel existente deve encaminhar esse hostname para http://127.0.0.1:8787.' : 'URL temporária: muda quando o túnel é reiniciado.');
    console.log('Compartilhe URL e chave em privado. A chave não é exibida nos logs. Ctrl+C para encerrar.');
    await new Promise((accept, reject) => {
      if (server.exitCode !== null) { if (server.exitCode && !stopping) process.exitCode = server.exitCode; return accept(); }
      server.once('exit', code => { if (code && !stopping) process.exitCode = code; accept(); });
      server.once('error', reject);
    });
  } finally {
    stop();
    if (temp) await rm(temp, { recursive: true, force: true });
    process.removeListener('SIGINT', stop); process.removeListener('SIGTERM', stop);
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch(error => { console.error(error.message); process.exitCode = 1; });
}
