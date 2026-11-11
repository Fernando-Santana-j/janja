#!/usr/bin/env python3
"""Collect a real interactive session, without SDP, ICE credentials or media."""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parent.parent
# Never export arbitrary signaling records or WebRTC stats strings.
EVENTS = {'startup', 'capture-ready', 'capture-metrics', 'peer-metrics',
          'pipeline', 'caps', 'gstreamer', 'latency-error', 'latency-updated',
          'engine-error', 'encoder-attempt', 'h264-recovery', 'bridge-push-error'}
SECRET = re.compile(r'ice[-_]pwd|ice[-_]ufrag|password|authorization|token|candidate:|a=fingerprint', re.I)


def clean(value):
    if isinstance(value, dict):
        return {k: '<REDACTED>' if SECRET.search(k) else clean(v) for k, v in value.items()}
    if isinstance(value, list):
        return [clean(v) for v in value]
    if isinstance(value, str):
        return '<REDACTED>' if SECRET.search(value) else value.replace(str(Path.home()), '~')
    return value


def command(args):
    try:
        p = subprocess.run(args, capture_output=True, text=True, timeout=4)
        return {'exit': p.returncode, 'out': p.stdout[:65536], 'error': p.stderr[:2048]}
    except (OSError, subprocess.TimeoutExpired) as e:
        return {'error': str(e)}


def audio():
    result = {}
    for kind in ('sinks', 'sources', 'sink-inputs', 'source-outputs'):
        reply = command(['pactl', '--format=json', 'list', kind])
        try:
            rows = json.loads(reply.get('out', ''))
            result[kind] = [{k: row[k] for k in (
                'index', 'name', 'state', 'sink', 'source', 'mute', 'volume',
                'monitor_source', 'monitor_of_sink', 'sample_specification') if k in row}
                for row in rows]
        except (ValueError, TypeError):
            result[kind] = reply
    result['default_sink'] = command(['pactl', 'get-default-sink'])
    return result


def records(paths):
    result = []
    for path in paths:
        for line in path.read_text(errors='replace').splitlines():
            try:
                record = json.loads(line)
                if isinstance(record, dict) and record.get('event') in EVENTS:
                    result.append(clean(record))
            except ValueError:
                pass
    return result


def main():
    binary = ROOT / 'dist/janja-linux'
    if not binary.is_file():
        raise SystemExit(f'Executável ausente: {binary}')
    folder = Path(tempfile.mkdtemp(prefix='janja-diagnostico-'))
    env = dict(os.environ, JANJA_LOG_DIR=str(folder / 'logs'))
    report = {'binary_sha256': hashlib.file_digest(binary.open('rb'), 'sha256').hexdigest(),
              'session': {k: os.environ.get(k) for k in ('XDG_SESSION_TYPE', 'XDG_CURRENT_DESKTOP')},
              'gpu': command(['lspci', '-nn']), 'audio_samples': []}
    print('No Janja que abrir: transmita uma janela EM MOVIMENTO com som por 30 segundos;')
    print('assista também à transmissão do Windows por 30 segundos. Depois FECHE o Janja.', flush=True)
    with (folder / 'terminal-local.txt').open('w') as output:
        process = subprocess.Popen([str(binary)], env=env, stdout=output, stderr=output)
        try:
            while process.poll() is None:
                sample = {'at_ms': int(time.time() * 1000), 'audio': audio()}
                try:
                    sample['process'] = [line for line in Path(f'/proc/{process.pid}/status').read_text().splitlines()
                                         if line.startswith(('VmRSS:', 'VmHWM:', 'Threads:'))]
                except OSError:
                    pass
                report['audio_samples'].append(sample)
                # Bound the collection if the application is left open overnight.
                report['audio_samples'] = report['audio_samples'][-120:]
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    pass
        except KeyboardInterrupt:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
    report['exit'] = process.returncode
    report['events'] = records(sorted((folder / 'logs').glob('*.jsonl')))
    destination = folder / 'relatorio.json'
    destination.write_text(json.dumps(clean(report), ensure_ascii=False, indent=2))
    print(f'Relatório para enviar: {destination}')
    print('Não envie terminal-local.txt nem os logs brutos.')


def self_test():
    assert clean({'ice-pwd': 'secret', 'nested': ['caps ice-ufrag=secret', '1080p']}) == {
        'ice-pwd': '<REDACTED>', 'nested': ['<REDACTED>', '1080p']}
    with tempfile.TemporaryDirectory() as directory:
        log = Path(directory) / 'log'
        log.write_text('\n'.join([json.dumps({'event': 'capture-metrics', 'data': {'fps': 30}}),
                                  json.dumps({'event': 'sdp', 'data': 'secret'}), '{broken', 'null']))
        assert records([log]) == [{'event': 'capture-metrics', 'data': {'fps': 30}}]
    print('Coletor: testes passaram.')


if __name__ == '__main__':
    self_test() if '--self-test' in sys.argv else main()
