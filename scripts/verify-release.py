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


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--directory', type=Path, default=Path('dist'))
    args = parser.parse_args()
    metadata = verify(Path(__file__).resolve().parent.parent, args.directory)
    print('Assinatura, identidade e ambos os instaladores conferidos:', metadata['version'])
