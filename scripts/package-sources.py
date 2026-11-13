#!/usr/bin/env python3
"""Archive only development sources and public assets; never credentials or build output."""
from pathlib import Path
import zipfile


def main():
    root = Path(__file__).resolve().parent.parent
    files = [root / name for name in ('Cargo.toml', 'Cargo.lock', 'build.rs', 'README.md', 'DESIGN.md')]
    for folder in ('src', 'scripts', 'assets', '.github', 'examples'):
        files.extend(p for p in (root / folder).rglob('*')
                     if p.is_file() and '__pycache__' not in p.parts and p.suffix != '.pyc')
    files.extend(p for p in (root / 'docs').glob('*.md') if p.name in
                 ('cliente-nativo.md', 'plano-r12.md', 'atualizacoes.md', 'auditoria-20261112.md'))
    dist = root / 'dist'
    dist.mkdir(exist_ok=True)
    archive = dist / 'janja-nativo-fontes.zip'
    pending = archive.with_suffix('.pending.zip')
    with zipfile.ZipFile(pending, 'w', zipfile.ZIP_DEFLATED, compresslevel=6) as output:
        for path in sorted(set(files)):
            if path.name.endswith(('.pem', '.xml')) and path.name not in ('update-public.pem', 'update-public.xml'):
                raise ValueError(f'Chave nao publica nos fontes: {path.name}')
            output.write(path, path.relative_to(root))
    with zipfile.ZipFile(pending) as check:
        assert check.testzip() is None
        assert not any('.release-keys' in name or 'update-private' in name for name in check.namelist())
    pending.replace(archive)
    print(f'Fontes: {archive} ({len(set(files))} arquivos)')


if __name__ == '__main__':
    main()
