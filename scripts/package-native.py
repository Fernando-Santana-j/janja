#!/usr/bin/env python3
"""Bundle native libraries and build one self-extracting executable. No web runtime."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import zipfile

PLUGINS = (
    "coreelements videotestsrc audiotestsrc app autodetect playback typefindfunctions videoconvertscale videoconvert videoscale "
    "videorate ximagesrc pipewire pulseaudio opus rtp rtpmanager webrtc dtls srtp nice "
    "videoparsersbad openh264 libav va vaapi nvcodec x264 qsv amfcodec mediafoundation "
    "d3d11 d3d12 wasapi wasapi2 directsound audioconvert audioresample "
    "opengl ximagesink xvimagesink waylandsink alsa volume"
).split()
SYSTEM_LINUX = re.compile(r"^(ld-linux|lib(c|m|dl|pthread|rt|resolv|nss_.*|EGL|GL|GLX|GLdispatch|OpenGL|drm.*|vulkan))\.so")
def output(*args):
    return subprocess.check_output(args, text=True, stderr=subprocess.STDOUT).strip()

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--offline", action="store_true")
    parser.add_argument("--prefix", type=Path)
    parser.add_argument("--diagnostics", action="store_true",
                        help="preservar também um executável separado com logs e autotestes")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    os.chdir(root)
    windows = sys.platform == "win32"
    if windows and shutil.which("makensis") is None:
        raise RuntimeError("Instale mingw-w64-ucrt-x86_64-nsis no MSYS2 UCRT64 para gerar o instalador.")
    prefix = args.prefix or Path(output("pkg-config", "--variable=prefix", "gstreamer-1.0"))
    libdir = Path(output("pkg-config", "--variable=libdir", "gstreamer-1.0"))
    plugin_dir = libdir / "gstreamer-1.0"
    ext = ".exe" if windows else ""
    cargo = ["cargo", "build", "--release", "--locked"]
    if args.offline:
        cargo.append("--offline")
    subprocess.run(cargo + ["--features", "native,diagnostics", "--bin", "janja"], check=True)
    files = {}
    queued = []
    def add(source, destination):
        source = Path(source)
        if not source.is_file():
            raise RuntimeError(f"Arquivo ausente: {source}")
        destination = str(destination).replace("\\", "/")
        if destination not in files:
            files[destination] = source
            if source.suffix.lower() in (".dll", ".exe") or re.search(r"\.so(?:\.\d+)*$", source.name) or source.name in ("janja", "gst-plugin-scanner"):
                queued.append(source)
    add(root / "target/release" / ("janja" + ext), "bin/janja" + ext)
    add(root / "assets/janja.png", "share/icons/hicolor/256x256/apps/br.com.infocoretech.janja.png")
    for name in PLUGINS:
        candidates = list(plugin_dir.glob("*gst" + name + (".dll" if windows else ".so")))
        for p in candidates:
            add(p, "plugins/" + p.name)
    for required in ["webrtc", "nice", "openh264", "libav", "opus", "app"]:
        if not any(re.search(r"gst" + required + r"\.(so|dll)$", name) for name in files):
            raise RuntimeError(f"Plugin obrigatório ausente: {required}")
    scanners = [libdir / "gstreamer-1.0" / ("gst-plugin-scanner" + ext),
                prefix / "libexec/gstreamer-1.0" / ("gst-plugin-scanner" + ext)]
    scanner = next((p for p in scanners if p.is_file()), None)
    if scanner is None:
        raise RuntimeError("gst-plugin-scanner ausente")
    add(scanner, "bin/gst-plugin-scanner" + ext)
    for folder in ["share/glib-2.0/schemas", "share/licenses", "etc/fonts"]:
        base = prefix / folder
        if base.is_dir():
            for p in base.rglob("*"):
                if p.is_file():
                    add(p, p.relative_to(prefix))
    if not windows:
        for name in ["spa-0.2", "pipewire-0.3"]:
            base = libdir / name
            if base.is_dir():
                for p in base.rglob("*"):
                    if p.is_file():
                        add(p, Path("lib") / p.relative_to(libdir))
    seen = set()
    dlls = {p.name.lower(): p for p in (prefix / "bin").glob("*.dll")} if windows else {}
    while queued:
        binary = queued.pop()
        if binary.resolve() in seen:
            continue
        seen.add(binary.resolve())
        if windows:
            dependencies = re.findall(r"DLL Name:\s*(\S+)", output("objdump", "-p", str(binary)))
            for name in dependencies:
                if name.lower() in dlls:
                    add(dlls[name.lower()], "bin/" + dlls[name.lower()].name)
                elif not (Path(os.environ["SYSTEMROOT"]) / "System32" / name).exists() and not name.lower().startswith(("api-ms-", "ext-ms-")):
                    raise RuntimeError(f"DLL não encontrada: {name} ({binary.name})")
        else:
            text = output("ldd", str(binary))
            if "not found" in text:
                raise RuntimeError(text)
            for name, source in re.findall(r"^\s*(\S+)\s+=>\s+(/\S+)", text, re.M):
                if not SYSTEM_LINUX.match(name):
                    add(Path(source), "lib/" + name)
    dist = root / "dist"
    dist.mkdir(exist_ok=True)
    bundle = dist / "janja-runtime.zip"
    def write_bundle():
        with zipfile.ZipFile(bundle, "w", zipfile.ZIP_DEFLATED, compresslevel=6) as archive:
            for destination, source in sorted(files.items()):
                archive.write(source, destination)
    write_bundle()
    env = dict(os.environ, JANJA_BUNDLE=str(bundle))
    subprocess.run(cargo + ["--features", "portable,diagnostics", "--bin", "janja-portable"], env=env, check=True)
    artifact = dist / ("janja-windows.exe" if windows else "janja-linux")
    staged = artifact.with_name(artifact.stem + ".pending" + artifact.suffix)
    shutil.copy2(root / "target/release" / ("janja-portable" + ext), staged)
    def check_launcher():
        if windows:
            for name in re.findall(r"DLL Name:\s*(\S+)", output("objdump", "-p", str(staged))):
                if not (Path(os.environ["SYSTEMROOT"]) / "System32" / name).exists() and not name.lower().startswith(("api-ms-", "ext-ms-")):
                    raise RuntimeError(f"O launcher ainda depende de uma DLL externa: {name}")
    check_launcher()
    with tempfile.TemporaryDirectory(prefix="janja-check-") as temporary:
        check_env = dict(os.environ)
        check_env["LOCALAPPDATA" if windows else "XDG_CACHE_HOME"] = temporary
        check_env["JANJA_LOG_DIR"] = str(Path(temporary) / "logs")
        subprocess.run([str(staged), "--check"], env=check_env, check=True, timeout=120)
        subprocess.run([str(staged), "--self-test"], env=check_env, check=True, timeout=120)
    if args.diagnostics:
        diagnostic = dist / ("janja-diagnostico-windows.exe" if windows else "janja-diagnostico-linux")
        diagnostic_staged = diagnostic.with_name(diagnostic.stem + ".pending" + diagnostic.suffix)
        shutil.copy2(staged, diagnostic_staged)
    # Keep diagnostics separate from the application distributed to end users.
    for name in list(files):
        if "audiotestsrc" in name or (name.startswith("lib/spa-0.2/") and "videotestsrc" in name):
            del files[name]
    subprocess.run(cargo + ["--features", "native", "--bin", "janja"], check=True)
    clean_binary = (root / "target/release" / ("janja" + ext)).read_bytes()
    if any(marker in clean_binary for marker in (b"--self-test", b"--network-test", b"diagnostics-overflow")):
        raise RuntimeError("O cliente final ainda contem codigo de diagnostico")
    write_bundle()
    subprocess.run(cargo + ["--features", "portable", "--bin", "janja-portable"], env=env, check=True)
    shutil.copy2(root / "target/release" / ("janja-portable" + ext), staged)
    check_launcher()
    with tempfile.TemporaryDirectory(prefix="janja-final-") as temporary:
        final_env=dict(os.environ)
        final_env["LOCALAPPDATA" if windows else "XDG_CACHE_HOME"]=temporary
        subprocess.run([str(staged),"--version"],env=final_env,check=True,timeout=120)
    with zipfile.ZipFile(bundle) as archive:
        if any(name.startswith(("src/","scripts/","tests/","logs/")) or name.endswith((".py",".ps1",".rs",".jsonl")) for name in archive.namelist()):
            raise RuntimeError("Arquivo de desenvolvimento dentro do pacote final")
    # Atomic replacement also works while the previous Linux executable is open.
    staged.replace(artifact)
    print(f"Gerado: {artifact} ({artifact.stat().st_size / 1024**2:.1f} MiB)")
    if windows:
        subprocess.run(["makensis", "/V3", "/WX", "/INPUTCHARSET", "UTF8", str(root / "scripts/installer-windows.nsi")], check=True)
        installer = dist / "janja-setup-windows.exe"
        (dist / "janja-setup-windows.pending.exe").replace(installer)
        print(f"Instalador: {installer} ({installer.stat().st_size / 1024**2:.1f} MiB)")
    else:
        subprocess.run([sys.executable, str(root / "scripts/package-linux-installer.py")], check=True)
    if args.diagnostics:
        diagnostic_staged.replace(diagnostic)
        print(f"Diagnóstico opcional: {diagnostic} ({diagnostic.stat().st_size / 1024**2:.1f} MiB)")
    version = re.search(r'pub const BUILD: &str = "([^"]+)";', (root / 'src/lib.rs').read_text()).group(1)
    installer_name = 'janja-setup-windows.exe' if windows else 'janja-setup-linux.sh'
    digests = {}
    for path in [artifact, dist / installer_name]:
        with path.open('rb') as stream:
            digests[path.name] = hashlib.file_digest(stream, 'sha256').hexdigest()
    (dist / ('windows.build.json' if windows else 'linux.build.json')).write_text(
        json.dumps({'version': version, 'sha256': digests}, sort_keys=True))
    print("Teste este arquivo em outra máquina antes de distribuir.")

if __name__ == "__main__":
    main()
