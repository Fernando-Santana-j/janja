#!/usr/bin/env python3
"""Installed launcher. Runs before the native client, never during its session."""
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.request

LIMIT = 512 * 1024**2
VERSION = re.compile(r'\d{4}\.\d{2}\.\d{2}-r(\d+)-[a-z0-9-]+')


def verified_manifest(payload, signature, public_key, repository):
    if payload.stat().st_size > 65536 or signature.stat().st_size != 384:
        raise ValueError('Invalid metadata size')
    subprocess.run(['openssl', 'dgst', '-sha256', '-verify', str(public_key),
                    '-signature', str(signature), str(payload)],
                   check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    metadata = json.loads(payload.read_bytes())
    match = VERSION.fullmatch(metadata.get('version', ''))
    if (metadata.get('schema') != 1 or metadata.get('repository') != repository or not match
            or metadata.get('revision') != int(match[1])):
        raise ValueError('Invalid update identity')
    asset = metadata['files']['linux-x86_64']
    if (asset['name'] != 'janja-setup-linux.sh' or not re.fullmatch(r'[a-f0-9]{64}', asset['sha256'])
            or type(asset['size']) is not int or not 0 < asset['size'] <= LIMIT):
        raise ValueError('Invalid update asset')
    return metadata


def verify_file(path, asset):
    hasher = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            hasher.update(block)
    digest = hasher.hexdigest()
    if path.stat().st_size != asset['size'] or digest != asset['sha256']:
        raise ValueError('Incomplete or modified update')


def download(url, path, limit, seconds, progress=None):
    deadline = time.monotonic() + seconds
    request = urllib.request.Request(url, headers={'User-Agent': 'Janja-Updater/1'})
    with urllib.request.urlopen(request, timeout=4) as response, path.open('wb') as output:
        if not response.url.startswith('https://'):
            raise ValueError('HTTPS required')
        size = int(response.headers.get('Content-Length', '-1'))
        if size > limit:
            raise ValueError('Update exceeds limit')
        total = 0
        while True:
            if time.monotonic() > deadline or (progress and progress.poll() is not None):
                raise TimeoutError('Update cancelled or timed out')
            block = response.read(65536)
            if not block:
                break
            total += len(block)
            if total > limit:
                raise ValueError('Update exceeds limit')
            output.write(block)
            if progress:
                progress.stdin.write(f'{min(99, total * 100 // limit)}\n')
                progress.stdin.flush()


def update(directory):
    config = json.loads((directory / 'update-config.json').read_text())
    repository = config['repository']
    if not re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', repository):
        raise ValueError('Invalid repository')
    current = (directory / 'version.txt').read_text().strip()
    revision = VERSION.fullmatch(current)
    if not revision:
        raise ValueError('Invalid installed version')
    with tempfile.TemporaryDirectory(prefix='.update-', dir=directory) as temporary:
        staging = Path(temporary)
        base = f'https://github.com/{repository}/releases/latest/download/'
        payload, signature = staging / 'manifest.json', staging / 'manifest.sig'
        download(base + 'janja-update.json', payload, 65536, 4)
        download(base + 'janja-update.json.sig', signature, 1024, 4)
        metadata = verified_manifest(payload, signature, directory / 'update-public.pem', repository)
        if metadata['revision'] <= int(revision[1]):
            return
        asset = metadata['files']['linux-x86_64']
        progress = subprocess.Popen(['zenity', '--progress', '--title=Atualizando Janja',
                                     '--text=Baixando uma nova versão. Feche esta janela para abrir a versão atual.',
                                     '--percentage=0', '--auto-close'], stdin=subprocess.PIPE, text=True,
                                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            installer = staging / 'setup.sh'
            download(f'https://github.com/{repository}/releases/download/{metadata["version"]}/{asset["name"]}',
                     installer, asset['size'], 600, progress)
            verify_file(installer, asset)
            if progress.poll() is not None:
                raise InterruptedError('Update cancelled')
            owned = ['janja-linux', 'update.py', 'launch', 'version.txt', 'update-config.json', 'update-public.pem']
            backup = staging / 'backup'
            backup.mkdir()
            for name in owned:
                shutil.copy2(directory / name, backup / name)
            try:
                subprocess.run(['/bin/sh', str(installer), '--update'], check=True,
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                if (directory / 'version.txt').read_text().strip() != metadata['version']:
                    raise ValueError('Update did not install the expected version')
            except Exception:
                for name in owned:
                    shutil.copy2(backup / name, directory / name)
                raise
        finally:
            progress.stdin.close()
            if progress.poll() is None:
                progress.terminate()
            progress.wait()


def main():
    directory = Path(__file__).resolve().parent
    executable = directory / 'janja-linux'
    if len(sys.argv) == 1:
        with (directory / '.update.lock').open('a') as lock:
            try:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                return
            try:
                update(directory)
            except Exception:
                # Offline, invalid or cancelled updates must not prevent opening Janja.
                pass
    os.environ['JANJA_INSTALLED_LAUNCHER'] = str(directory / 'launch')
    os.execv(executable, [str(executable), *sys.argv[1:]])


if __name__ == '__main__':
    main()
