#!/usr/bin/env python3
"""Collect a real interactive session, without SDP, ICE credentials or media."""
import hashlib
from collections import deque
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
          'engine-error', 'encoder-attempt', 'h264-recovery', 'bridge-push-error', 'quality-adapted'}
SECRET = re.compile(r'ice[-_]pwd|ice[-_]ufrag|password|credential|authorization|token|candidate:|a=fingerprint|turns?://[^\s]*@', re.I)
MAX_EVENTS = 20000


def diagnostic_binary(root):
    binary = root / 'dist/janja-diagnostico-linux'
    if not binary.is_file():
        raise SystemExit(f'Executável de diagnóstico ausente: {binary}\n'
                         'Gere-o com python3 scripts/package-native.py --diagnostics '
                         'ou baixe o artefato de diagnóstico do CI. O cliente comum não grava logs.')
    return binary


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
    result = deque(maxlen=MAX_EVENTS)
    for path in paths:
        with path.open(errors='replace') as source:
            for line in source:
                try:
                    record = json.loads(line)
                    if isinstance(record, dict) and record.get('event') in EVENTS:
                        result.append(clean(record))
                except ValueError:
                    pass
    if not result:
        raise ValueError('O aplicativo não gerou eventos de diagnóstico. '
                         'Use janja-diagnostico-linux gerado com --diagnostics.')
    return list(result)


def process_resources(stat, now, clock_ticks, page_size, previous=None):
    # The process name may contain spaces and closing parentheses.
    fields = stat.rsplit(')', 1)[1].split()
    result = {'sampled_at': now,
              'cpu_seconds': (int(fields[11]) + int(fields[12])) / clock_ticks,
              'rss_bytes': int(fields[21]) * page_size,
              'threads': int(fields[17]), 'cpu_percent': None}
    if previous is not None and now > previous['sampled_at']:
        result['cpu_percent'] = round(max(0, result['cpu_seconds'] - previous['cpu_seconds'])
                                      * 100 / (now - previous['sampled_at']), 2)
    return result


def main():
    binary = diagnostic_binary(ROOT)
    folder = Path(tempfile.mkdtemp(prefix='janja-diagnostico-'))
    env = dict(os.environ, JANJA_LOG_DIR=str(folder / 'logs'))
    report = {'binary_sha256': hashlib.file_digest(binary.open('rb'), 'sha256').hexdigest(),
              'session': {k: os.environ.get(k) for k in ('XDG_SESSION_TYPE', 'XDG_CURRENT_DESKTOP')},
              'gpu': command(['lspci', '-nn']), 'audio_samples': []}
    print('No Janja que abrir: transmita uma janela EM MOVIMENTO com som por 30 segundos;')
    print('assista também à transmissão do Windows por 30 segundos. Depois FECHE o Janja.', flush=True)
    with (folder / 'terminal-local.txt').open('w') as output:
        process = subprocess.Popen([str(binary)], env=env, stdout=output, stderr=output)
        previous = None
        try:
            while process.poll() is None:
                sample = {'at_ms': int(time.time() * 1000), 'audio': audio()}
                try:
                    sample['process'] = process_resources(
                        Path(f'/proc/{process.pid}/stat').read_text(), time.monotonic(),
                        os.sysconf('SC_CLK_TCK'), os.sysconf('SC_PAGE_SIZE'), previous)
                    previous = sample['process']
                except (OSError, ValueError, IndexError):
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
    try:
        report['events'] = records(sorted((folder / 'logs').glob('*.jsonl')))
    except ValueError as error:
        raise SystemExit(f'{error}\nCódigo de saída: {process.returncode}. '
                         f'Confira localmente {folder / "terminal-local.txt"}; não envie esse arquivo.') from None
    destination = folder / 'relatorio.json'
    destination.write_text(json.dumps(clean(report), ensure_ascii=False, indent=2))
    print(f'Relatório para enviar: {destination}')
    print('Não envie terminal-local.txt nem os logs brutos.')


def self_test():
    assert clean({'relay': 'turns://user:relay-secret@relay.invalid:5349'}) == {'relay': '<REDACTED>'}
    assert clean({'ice-pwd': 'secret', 'nested': ['caps ice-ufrag=secret', '1080p']}) == {
        'ice-pwd': '<REDACTED>', 'nested': ['<REDACTED>', '1080p']}
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        (root / 'dist').mkdir()
        (root / 'dist/janja-linux').touch()
        try:
            diagnostic_binary(root)
        except SystemExit as error:
            assert '--diagnostics' in str(error)
        else:
            raise AssertionError('O coletor selecionou o cliente sem diagnóstico')
        binary = root / 'dist/janja-diagnostico-linux'
        binary.touch()
        assert diagnostic_binary(root) == binary
        log = Path(directory) / 'log'
        log.write_text('\n'.join([json.dumps({'event': 'capture-metrics', 'data': {'fps': 30}}),
                                  json.dumps({'event': 'sdp', 'data': 'secret'}), '{broken', 'null']))
        assert records([log]) == [{'event': 'capture-metrics', 'data': {'fps': 30}}]
        try:
            records([])
        except ValueError:
            pass
        else:
            raise AssertionError('O coletor aceitou um relatório sem eventos de diagnóstico')
        with log.open('w') as output:
            for index in range(MAX_EVENTS + 2):
                output.write(json.dumps({'event': 'capture-metrics', 'index': index,
                                         'data': {'token': 'secret'}}) + '\n')
        history = records([log])
        assert len(history) == MAX_EVENTS and history[0]['index'] == 2
        assert history[-1]['data']['token'] == '<REDACTED>'
    fields = ['0'] * 22
    fields[0], fields[11], fields[12], fields[17], fields[21] = 'R', '80', '20', '4', '3'
    first = process_resources('123 (Janja ) capture) ' + ' '.join(fields), 10, 100, 4096)
    assert first['rss_bytes'] == 12288 and first['threads'] == 4
    assert first['cpu_seconds'] == 1 and first['cpu_percent'] is None
    fields[11], fields[12] = '160', '40'
    second = process_resources('123 (Janja) ' + ' '.join(fields), 12, 100, 4096, first)
    assert second['cpu_percent'] == 50
    assert process_resources('123 (Janja) ' + ' '.join(fields), 12, 100, 4096, second)['cpu_percent'] is None
    print('Coletor: testes passaram.')


if __name__ == '__main__':
    self_test() if '--self-test' in sys.argv else main()
