#!/usr/bin/env python3
"""Offline tests: signed metadata, bounded downloads and per-user Linux installation."""
import copy
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parent.parent


def load(name, file):
    spec = importlib.util.spec_from_file_location(name, ROOT / 'scripts' / file)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


update = load('update_linux', 'update-linux.py')
release = load('release_manifest', 'release-manifest.py')
published = load('verify_release', 'verify-release.py')


class Updates(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory(prefix='janja-update-tests-')
        cls.root = Path(cls.temporary.name)
        cls.private = cls.root / 'private.pem'
        cls.public = cls.root / 'public.pem'
        subprocess.run(['openssl', 'genpkey', '-algorithm', 'RSA', '-pkeyopt', 'rsa_keygen_bits:3072',
                        '-out', str(cls.private)], check=True, stderr=subprocess.DEVNULL)
        subprocess.run(['openssl', 'pkey', '-in', str(cls.private), '-pubout', '-out', str(cls.public)],
                       check=True, stderr=subprocess.DEVNULL)

    @classmethod
    def tearDownClass(cls):
        cls.temporary.cleanup()

    def setUp(self):
        self.directory = Path(tempfile.mkdtemp(dir=self.root))
        self.payload = self.directory / 'manifest.json'
        self.signature = self.directory / 'manifest.sig'
        self.asset = self.directory / 'setup.sh'
        self.asset.write_bytes(b'#!/bin/sh\nexit 0\n')
        self.metadata = {'schema': 1, 'repository': 'owner/app', 'version': '2026.11.13-r12-leve', 'revision': 12,
                         'files': {'linux-x86_64': {'name': 'janja-setup-linux.sh', 'size': self.asset.stat().st_size,
                         'sha256': hashlib.sha256(self.asset.read_bytes()).hexdigest()}}}

    def sign(self, metadata):
        self.payload.write_text(json.dumps(metadata, sort_keys=True, separators=(',', ':')))
        subprocess.run(['openssl', 'dgst', '-sha256', '-sign', str(self.private), '-out',
                        str(self.signature), str(self.payload)], check=True, stderr=subprocess.DEVNULL)

    def verify(self):
        return update.verified_manifest(self.payload, self.signature, self.public, 'owner/app')

    def linux_setup(self, path, version, binary_version=None):
        with path.open('wb') as stream:
            stream.write((ROOT / 'scripts/installer-linux.sh').read_bytes())
            with tarfile.open(fileobj=stream, mode='w:gz') as archive:
                files = {'janja-linux': f'#!/bin/sh\necho "Janja {binary_version or version}"\n'.encode(),
                         'version.txt': version.encode(), 'launch': b'#!/bin/sh\nexit 0\n',
                         'update.py': b'', 'update-config.json': b'{"repository":"owner/app"}',
                         'update-public.pem': self.public.read_bytes()}
                for name, data in files.items():
                    info = tarfile.TarInfo(name)
                    info.size = len(data)
                    info.mode = 0o755
                    archive.addfile(info, io.BytesIO(data))

    def test_valid_signed_manifest_and_artifact(self):
        self.sign(self.metadata)
        metadata = self.verify()
        update.verify_file(self.asset, metadata['files']['linux-x86_64'])

    def test_modified_metadata_and_signature_are_rejected(self):
        self.sign(self.metadata)
        self.payload.write_bytes(self.payload.read_bytes() + b' ')
        with self.assertRaises(subprocess.CalledProcessError):
            self.verify()
        self.sign(self.metadata)
        signature = bytearray(self.signature.read_bytes())
        signature[0] ^= 1
        self.signature.write_bytes(signature)
        with self.assertRaises(subprocess.CalledProcessError):
            self.verify()

    def test_invalid_identity_revision_size_and_paths_are_rejected(self):
        for field, value in [('schema', 2), ('repository', 'other/repo'), ('revision', 13), ('version', '../app')]:
            metadata = copy.deepcopy(self.metadata)
            metadata[field] = value
            self.sign(metadata)
            with self.subTest(field=field), self.assertRaises(ValueError):
                self.verify()
        for field, value in [('name', '../other.sh'), ('size', update.LIMIT + 1), ('size', 0), ('sha256', 'invalid')]:
            metadata = copy.deepcopy(self.metadata)
            metadata['files']['linux-x86_64'][field] = value
            self.sign(metadata)
            with self.subTest(field=field), self.assertRaises(ValueError):
                self.verify()

    def test_altered_and_incomplete_artifacts_are_rejected(self):
        asset = self.metadata['files']['linux-x86_64']
        self.asset.write_bytes(b'x' * asset['size'])
        with self.assertRaises(ValueError):
            update.verify_file(self.asset, asset)
        self.asset.write_bytes(b'')
        with self.assertRaises(ValueError):
            update.verify_file(self.asset, asset)

    def test_downloads_are_bounded_without_content_length(self):
        response = io.BytesIO(b'x' * 100)
        response.headers = {}
        response.url = 'https://github.com/owner/app'
        with patch.object(update.urllib.request, 'urlopen', return_value=response):
            with self.assertRaises(ValueError):
                update.download(response.url, self.directory / 'download', 64, 1)

    def test_release_manifest_requires_both_installers_and_uses_their_hashes(self):
        (self.directory / 'assets').mkdir()
        (self.directory / 'src').mkdir()
        (self.directory / 'assets/update-config.json').write_text('{"repository":"owner/app"}')
        (self.directory / 'src/lib.rs').write_text('pub const BUILD: &str = "2026.11.13-r12-leve";')
        with self.assertRaises(ValueError):
            release.manifest(self.directory, self.directory)
        for name in ['janja-setup-linux.sh', 'janja-setup-windows.exe']:
            (self.directory / name).write_bytes(b'test artifact')
            platform = 'windows' if name.endswith('.exe') else 'linux'
            (self.directory / f'{platform}.build.json').write_text(json.dumps({
                'version': '2026.11.13-r12-leve',
                'sha256': {name: hashlib.sha256(b'test artifact').hexdigest()}}))
        metadata = release.manifest(self.directory, self.directory)
        self.assertEqual(metadata['revision'], 12)
        self.assertEqual(metadata['files']['linux-x86_64']['sha256'], hashlib.sha256(b'test artifact').hexdigest())
        (self.directory / 'janja-setup-windows.exe').write_bytes(b'outdated artifact')
        with self.assertRaises(ValueError):
            release.manifest(self.directory, self.directory)

    def test_downloaded_release_uses_the_installed_key_and_rejects_modified_windows_setup(self):
        root = self.directory / 'source'
        (root / 'assets').mkdir(parents=True)
        (root / 'src').mkdir()
        shutil.copyfile(self.public, root / 'assets/update-public.pem')
        (root / 'assets/update-config.json').write_text('{"repository":"owner/app"}')
        (root / 'src/lib.rs').write_text('pub const BUILD: &str = "2026.11.13-r12-leve";')
        metadata = copy.deepcopy(self.metadata)
        for platform, name in [('windows-x86_64', 'janja-setup-windows.exe'), ('linux-x86_64', 'janja-setup-linux.sh')]:
            (self.directory / name).write_bytes(b'fixture installer')
            metadata['files'][platform] = {'name': name, 'size': len(b'fixture installer'),
                'sha256': hashlib.sha256(b'fixture installer').hexdigest()}
        self.sign(metadata)
        shutil.copyfile(self.payload, self.directory / 'janja-update.json')
        shutil.copyfile(self.signature, self.directory / 'janja-update.json.sig')
        self.assertEqual(published.verify(root, self.directory)['version'], metadata['version'])
        (self.directory / 'janja-setup-windows.exe').write_bytes(b'modified')
        with self.assertRaises(ValueError):
            published.verify(root, self.directory)

    def test_linux_installer_preserves_unknown_files_and_rejects_bad_binary(self):
        home = self.directory / 'Usuario com espacos'
        installed = home / '.local/share/janja'
        installed.mkdir(parents=True)
        sentinel = installed / 'arquivo-do-usuario.txt'
        sentinel.write_text('preservar')
        installer = self.directory / 'installer.sh'
        for version, expected in [('2026.11.13-r12-leve', 0), ('wrong-version', 1)]:
            self.linux_setup(installer, '2026.11.13-r12-leve', binary_version=version)
            result = subprocess.run(['/bin/sh', str(installer), '--update'],
                                    env=dict(os.environ, HOME=str(home)), capture_output=True)
            self.assertEqual(result.returncode, expected, result.stderr.decode())
            self.assertEqual(sentinel.read_text(), 'preservar')
            self.assertEqual((installed / 'version.txt').read_text(), '2026.11.13-r12-leve')

    def test_signed_update_flow_success_corruption_rollback_offline_and_cancel(self):
        owned = ['janja-linux', 'update.py', 'launch', 'version.txt', 'update-config.json', 'update-public.pem']
        original_popen = subprocess.Popen

        class Progress:
            def __init__(self, cancelled):
                self.stdin = io.StringIO()
                self.done = cancelled

            def poll(self):
                return 0 if self.done else None

            def terminate(self):
                self.done = True

            def wait(self):
                return 0

        for case in ['success', 'corrupt', 'rollback', 'offline', 'cancel', 'older']:
            with self.subTest(case=case):
                home = self.directory / case / 'Usuario com espacos'
                installed = home / '.local/share/janja'
                installed.mkdir(parents=True)
                for name in owned:
                    (installed / name).write_text('original')
                (installed / 'update-config.json').write_text('{"repository":"owner/app"}')
                (installed / 'version.txt').write_text('2026.11.13-r12-leve')
                shutil.copyfile(self.public, installed / 'update-public.pem')
                original = {name: (installed / name).read_bytes() for name in owned}
                profile = home / '.config/janja/profile.json'
                profile.parent.mkdir(parents=True)
                profile.write_text('{"name":"preservar"}')
                self.linux_setup(self.asset, '2026.11.14-r13-leve')
                if case == 'rollback':
                    self.asset.write_text('#!/bin/sh\nprintf broken > "$HOME/.local/share/janja/janja-linux"\nexit 1\n')
                metadata = copy.deepcopy(self.metadata)
                metadata.update(version='2026.11.14-r13-leve', revision=13)
                if case == 'older':
                    metadata.update(version='2026.11.12-r11-leve', revision=11)
                metadata['files']['linux-x86_64'].update(size=self.asset.stat().st_size,
                    sha256=hashlib.sha256(self.asset.read_bytes()).hexdigest())
                self.sign(metadata)
                if case == 'corrupt':
                    self.asset.write_bytes(self.asset.read_bytes() + b'alterado')

                def download(url, path, *_args):
                    if case == 'offline':
                        raise TimeoutError('offline fixture')
                    source = self.signature if url.endswith('.sig') else self.payload if url.endswith('.json') else self.asset
                    shutil.copyfile(source, path)

                def popen(arguments, *args, **kwargs):
                    return Progress(case == 'cancel') if arguments[0] == 'zenity' else original_popen(arguments, *args, **kwargs)

                with patch.object(update, 'download', side_effect=download), patch.object(subprocess, 'Popen', side_effect=popen), patch.dict(os.environ, HOME=str(home)):
                    if case in ['success', 'older']:
                        update.update(installed)
                    else:
                        with self.assertRaises((ValueError, TimeoutError, InterruptedError, subprocess.CalledProcessError)):
                            update.update(installed)
                self.assertEqual(profile.read_text(), '{"name":"preservar"}')
                if case == 'success':
                    self.assertEqual((installed / 'version.txt').read_text(), metadata['version'])
                else:
                    self.assertEqual({name: (installed / name).read_bytes() for name in owned}, original)

    def test_update_failure_still_launches_the_installed_client(self):
        installed = self.directory / 'installed'
        installed.mkdir()
        with patch.object(update, '__file__', str(installed / 'update.py')), patch.object(update.sys, 'argv', ['launch']), \
                patch.object(update, 'update', side_effect=TimeoutError('offline')), patch.object(update.os, 'execv') as launch, \
                patch.dict(os.environ):
            update.main()
            launch.assert_called_once_with(installed / 'janja-linux', [str(installed / 'janja-linux')])


if __name__ == '__main__':
    unittest.main()
