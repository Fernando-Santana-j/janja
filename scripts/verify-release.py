#!/usr/bin/env python3
"""Verify downloaded release installers against the public key shipped to users."""
import argparse
import importlib.util
import json
from pathlib import Path
import re

spec = importlib.util.spec_from_file_location('update_linux', Path(__file__).with_name('update-linux.py'))
update = importlib.util.module_from_spec(spec)
spec.loader.exec_module(update)


def verify(root, directory):
    repository = json.loads((root / 'assets/update-config.json').read_text())['repository']
    metadata = update.verified_manifest(directory / 'janja-update.json', directory / 'janja-update.json.sig',
                                        root / 'assets/update-public.pem', repository)
    version = re.search(r'pub const BUILD: &str = "([^"]+)";', (root / 'src/lib.rs').read_text()).group(1)
    if metadata['version'] != version:
        raise ValueError('A versao publicada difere do codigo validado')
    for platform, name in [('linux-x86_64', 'janja-setup-linux.sh'), ('windows-x86_64', 'janja-setup-windows.exe')]:
        asset = metadata['files'][platform]
        if (asset['name'] != name or type(asset['size']) is not int or not 0 < asset['size'] <= update.LIMIT
                or not re.fullmatch(r'[a-f0-9]{64}', asset['sha256'])):
            raise ValueError('Arquivo de publicacao invalido')
        update.verify_file(directory / name, asset)
    return metadata


def verify_builds(directory, version, validated):
    windows = {}
    for platform, names in [('linux', ['janja-linux', 'janja-setup-linux.sh']),
                            ('windows', ['janja-windows.exe', 'janja-setup-windows.exe'])]:
        stamp = json.loads((validated / f'{platform}.build.json').read_text())
        if stamp.get('version') != version:
            raise ValueError('O pacote local pertence a outra versao')
        for name in names:
            digest = stamp['sha256'][name]
            path = directory / name
            size = path.stat().st_size
            if not re.fullmatch(r'[a-f0-9]{64}', digest) or not 0 < size <= update.LIMIT:
                raise ValueError('Pacote local invalido')
            update.verify_file(path, {'size': size, 'sha256': digest})
            if platform == 'windows':
                windows[name] = digest
    report = json.loads((validated / 'windows-security.json').read_text(encoding='utf-8-sig'))
    scans = report['authenticode']
    if (report['defender'].get('status') != 'no_detection_in_scan' or report['defender'].get('exit_code') != 0
            or len(scans) != 2 or {scan['file']: scan['sha256'] for scan in scans} != windows):
        raise ValueError('O Defender nao aprovou os arquivos exatos desta publicacao')


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--directory', type=Path, default=Path('dist'))
    parser.add_argument('--validated-builds', type=Path,
                        help='conferir tambem portateis e hashes da analise local do Defender')
    args = parser.parse_args()
    metadata = verify(Path(__file__).resolve().parent.parent, args.directory)
    if args.validated_builds:
        verify_builds(args.directory, metadata['version'], args.validated_builds)
    print('Assinatura, identidade e ambos os instaladores conferidos:', metadata['version'])
