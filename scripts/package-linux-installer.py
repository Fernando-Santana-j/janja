#!/usr/bin/env python3
"""Bundle the installed Linux launcher separately from the media runtime."""
import io
from pathlib import Path
import re
import tarfile


def main():
    root = Path(__file__).resolve().parent.parent
    source = (root / 'src/lib.rs').read_text()
    version = re.search(r'pub const BUILD: &str = "([^"]+)";', source).group(1)
    files = {
        'janja-linux': root / 'dist/janja-linux',
        'update.py': root / 'scripts/update-linux.py',
        'update-config.json': root / 'assets/update-config.json',
        'update-public.pem': root / 'assets/update-public.pem',
    }
    header = (root / 'scripts/installer-linux.sh').read_bytes()
    output = root / 'dist/janja-setup-linux.sh'
    pending = output.with_suffix('.pending.sh')
    with pending.open('wb') as stream:
        stream.write(header)
        with tarfile.open(fileobj=stream, mode='w:gz') as archive:
            for name, path in files.items():
                archive.add(path, arcname=name, recursive=False)
            for name, data in {
                'version.txt': version.encode(),
                'launch': b'#!/bin/sh\nexec python3 "$(dirname "$0")/update.py" "$@"\n',
            }.items():
                info = tarfile.TarInfo(name)
                info.size = len(data)
                info.mode = 0o755 if name == 'launch' else 0o644
                archive.addfile(info, io.BytesIO(data))
    pending.chmod(0o755)
    pending.replace(output)
    print(f'Instalador Linux: {output} ({output.stat().st_size / 1024**2:.1f} MiB)')


if __name__ == '__main__':
    main()
