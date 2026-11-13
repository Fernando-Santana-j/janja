#!/usr/bin/env python3
"""Sign the exact release metadata; upload the signature and installers together."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess


def manifest(root, directory):
    repository = json.loads((root / 'assets/update-config.json').read_text())['repository']
    if not re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', repository):
        raise ValueError('Repositorio invalido')
    source = (root / 'src/lib.rs').read_text()
    version = re.search(r'pub const BUILD: &str = "([^"]+)";', source).group(1)
    revision = re.fullmatch(r'\d{4}\.\d{2}\.\d{2}-r(\d+)-[a-z0-9-]+', version)
    if not revision:
        raise ValueError('Versao invalida para atualizacao')
    files = {}
    for platform, name in [('windows-x86_64', 'janja-setup-windows.exe'), ('linux-x86_64', 'janja-setup-linux.sh')]:
        path = directory / name
        if not path.is_file():
            raise ValueError(f'Artefato ausente: {name}')
        size = path.stat().st_size
        if not 0 < size <= 512 * 1024**2:
            raise ValueError(f'Tamanho invalido: {name}')
        with path.open('rb') as stream:
            digest = hashlib.file_digest(stream, 'sha256').hexdigest()
        stamp = json.loads((directory / ('windows.build.json' if platform.startswith('windows') else 'linux.build.json')).read_text())
        if stamp.get('version') != version or stamp.get('sha256', {}).get(name) != digest:
            raise ValueError(f'Artefato antigo ou alterado: {name}')
        files[platform] = {'name': name, 'size': size, 'sha256': digest}
    return {'schema': 1, 'repository': repository, 'version': version,
            'revision': int(revision[1]), 'files': files}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--directory', type=Path, default=Path('dist'))
    parser.add_argument('--private-key', type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    metadata = manifest(root, args.directory)
    payload = args.directory / 'janja-update.json'
    signature = args.directory / 'janja-update.json.sig'
    payload.write_bytes(json.dumps(metadata, sort_keys=True, separators=(',', ':')).encode())
    subprocess.run(['openssl', 'dgst', '-sha256', '-sign', str(args.private_key),
                    '-out', str(signature), str(payload)], check=True)
    # Refuse a private key that does not match the key installed on users' machines.
    subprocess.run(['openssl', 'dgst', '-sha256', '-verify', str(root / 'assets/update-public.pem'),
                    '-signature', str(signature), str(payload)], check=True)
    print(f'Metadados assinados: {metadata["version"]}')


if __name__ == '__main__':
    main()
