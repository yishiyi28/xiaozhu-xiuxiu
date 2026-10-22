#!/usr/bin/env python3
"""Build native packages from the same staged files as the portable archive."""

import argparse
import hashlib
import os
import platform
import re
import shutil
import struct
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ARCHITECTURES = {"x86_64": ("amd64", 62), "aarch64": ("arm64", 183)}
DEB_LIBRARIES = [
    "libgcc-s1",
    "libvulkan1",
    "libxkbcommon0",
    "libxkbcommon-x11-0",
    "libwayland-client0",
    "libx11-6",
    "libx11-xcb1",
    "libxcb1",
    "libxcursor1",
    "libxi6",
    "libxrandr2",
    "libxfixes3",
]
RPM_LIBRARIES = [
    "libvulkan.so.1",
    "libxkbcommon.so.0",
    "libxkbcommon-x11.so.0",
    "libwayland-client.so.0",
    "libX11.so.6",
    "libX11-xcb.so.1",
    "libxcb.so.1",
    "libXcursor.so.1",
    "libXi.so.6",
    "libXrandr.so.2",
    "libXfixes.so.3",
]


def check_tools(package_format):
    commands = ["readelf"]
    if package_format in ("deb", "all"):
        commands.append("dpkg-deb")
    if package_format in ("rpm", "all"):
        commands.append("rpmbuild")
    if package_format in ("appimage", "all"):
        commands.extend(("linuxdeploy", "patchelf", "ldconfig"))
    missing = [command for command in commands if not shutil.which(command)]
    if missing:
        raise ValueError(f"Missing packaging tools: {', '.join(missing)}")
    if platform.machine() not in ARCHITECTURES:
        raise ValueError("Native packages support x86_64 and aarch64 Linux hosts")


def native_version(version):
    if not re.fullmatch(
        r"\d+\.\d+\.\d+(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?", version
    ):
        raise ValueError(f"Expected semver without build metadata: {version}")
    return version.replace("-", "~", 1)


def glibc_requirement(binary, architecture):
    with binary.open("rb") as executable:
        header = executable.read(20)
    if (
        header[:6] != b"\x7fELF\x02\x01"
        or struct.unpack("<H", header[18:20])[0] != ARCHITECTURES[architecture][1]
    ):
        raise ValueError(
            "The packaged executable must match the native host architecture"
        )
    versions = subprocess.check_output(
        ["readelf", "--version-info", "--wide", binary], text=True
    )
    glibc = re.findall(r"\bGLIBC_(\d+(?:\.\d+)+)\b", versions)
    if not glibc:
        raise ValueError("Expected a dynamically linked GNU/Linux executable")
    return max(glibc, key=lambda version: tuple(map(int, version.split("."))))


def stage_payload(stage, payload):
    prefix = payload / "usr"
    shutil.copytree(stage / "bin", prefix / "bin")
    shutil.copytree(stage / "share", prefix / "share")
    # Package files must stay readable even when the builder has a private umask.
    payload.chmod(0o755)
    for path in payload.rglob("*"):
        path.chmod(0o755 if path.is_dir() else 0o644)
    (prefix / "bin/xiaozhu-xiuxiu").chmod(0o755)


def build_deb(payload, output, version, architecture, glibc):
    # Dynamic desktop libraries are loaded with dlopen and do not appear in ELF NEEDED.
    dynamic = subprocess.check_output(
        ["readelf", "--dynamic", payload / "usr/bin/xiaozhu-xiuxiu"], text=True
    )
    needed = set(re.findall(r"Shared library: \[(.*?)\]", dynamic))
    known = {
        "libc.so.6",
        "libm.so.6",
        "libgcc_s.so.1",
        "libpthread.so.0",
        "libdl.so.2",
        "librt.so.1",
        "ld-linux-x86-64.so.2",
        "ld-linux-aarch64.so.1",
    }
    if needed - known:
        raise ValueError(
            f"Add Debian dependency mappings for: {', '.join(sorted(needed - known))}"
        )
    installed_size = sum(
        path.stat().st_size for path in payload.rglob("*") if path.is_file()
    )
    control = payload / "DEBIAN"
    control.mkdir()
    dependencies = ", ".join([f"libc6 (>= {glibc})", *DEB_LIBRARIES])
    (control / "control").write_text(
        f"Package: xiaozhu-xiuxiu\nVersion: {version}-1\n"
        f"Architecture: {ARCHITECTURES[architecture][0]}\n"
        "Section: graphics\nPriority: optional\n"
        "Maintainer: Silver Ling <silver.ling@outlook.com>\n"
        "Homepage: https://github.com/yishiyi28/xiaozhu-xiuxiu\n"
        f"Installed-Size: {(installed_size + 1023) // 1024}\nDepends: {dependencies}\n"
        "Recommends: xdg-desktop-portal\n"
        "Description: Native Linux image editor\n"
        " Layered compositions, photo retouching, and camera RAW development.\n"
    )
    for name in ("postinst", "postrm"):
        shutil.copy2(ROOT / "packaging/refresh-desktop.sh", control / name)
        (control / name).chmod(0o755)
    subprocess.run(
        ["dpkg-deb", "--root-owner-group", "-Zxz", "--build", payload, output],
        check=True,
    )
    shutil.rmtree(control)


def build_rpm(payload, output, version, architecture, temporary):
    spec = temporary / "xuan.spec"
    refresh = (ROOT / "packaging/refresh-desktop.sh").read_text().split("\n", 1)[1]
    requires = "\n".join(f"Requires: {library}()(64bit)" for library in RPM_LIBRARIES)
    spec.write_text(
        f"Name: xiaozhu-xiuxiu\nVersion: {version}\nRelease: 1\nBuildArch: {architecture}\n"
        "Summary: Native Linux image editor\n"
        "License: MIT AND LGPL-2.1-only AND OFL-1.1 AND Apache-2.0\n"
        "URL: https://github.com/yishiyi28/xiaozhu-xiuxiu\n"
        f"{requires}\nRecommends: xdg-desktop-portal\n"
        "\n%description\n"
        "Layered compositions, photo retouching, and camera RAW development.\n"
        '\n%install\nmkdir -p "%{buildroot}"\n'
        'cp -a "%{xuan_payload}/usr" "%{buildroot}/"\n'
        f"\n%post\n{refresh}\n%postun\n{refresh}"
        "\n%files\n%defattr(-,root,root,-)\n/usr/bin/xiaozhu-xiuxiu\n"
        "/usr/share/applications/com.xiaozhu.xiuxiu.desktop\n"
        "/usr/share/mime/packages/com.xiaozhu.xiuxiu.xml\n"
        "/usr/share/icons/hicolor/*/apps/com.xiaozhu.xiuxiu.png\n"
        "%doc /usr/share/doc/xiaozhu-xiuxiu\n%license /usr/share/licenses/xiaozhu-xiuxiu\n"
    )
    subprocess.run(
        [
            "rpmbuild",
            "-bb",
            spec,
            "--define",
            f"_topdir {temporary / 'rpm'}",
            "--define",
            f"_tmppath {temporary}",
            "--define",
            f"_rpmdir {output.parent}",
            "--define",
            f"_rpmfilename {output.name}",
            "--define",
            f"xuan_payload {payload}",
            "--define",
            "debug_package %{nil}",
            "--define",
            "_build_id_links none",
        ],
        check=True,
    )


def copy_rpm_library_licenses(payload, cache):
    # linuxdeploy collects Debian copyright files, but has no RPM backend.
    if not Path("/etc/redhat-release").exists():
        return
    for library in (payload / "usr/lib").glob("*.so.*"):
        original = re.search(
            rf"^\s*{re.escape(library.name)} .* => (.+)$", cache, re.MULTILINE
        )
        package = subprocess.check_output(
            ["rpm", "-qf", "--qf", "%{NAME}", original[1]], text=True
        )
        # These runtime packages share notices with their common/base package.
        package = {
            "libX11": "libX11-common",
            "libX11-xcb": "libX11-common",
            "libxkbcommon-x11": "libxkbcommon",
        }.get(package, package)
        files = subprocess.check_output(["rpm", "-ql", package], text=True).splitlines()
        notices = [
            Path(name)
            for name in files
            if "/licenses/" in name
            or Path(name).name in ("COPYING", "LICENSE", "copyright")
        ]
        if not any(path.is_file() for path in notices):
            raise ValueError(f"Missing AppImage library license notices: {package}")
        for notice in notices:
            if notice.is_file():
                destination = payload / notice.relative_to("/")
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(notice, destination)


def find_library(name, cache, architecture):
    for candidate in re.findall(
        rf"^\s*{re.escape(name)} .* => (.+)$", cache, re.MULTILINE
    ):
        path = Path(candidate)
        with path.open("rb") as library:
            header = library.read(20)
        if (
            header[:6] == b"\x7fELF\x02\x01"
            and struct.unpack("<H", header[18:20])[0] == ARCHITECTURES[architecture][1]
        ):
            return path
    raise ValueError(f"Missing AppImage runtime library: {name}")


def bundle_glibc(payload):
    # Run as a linuxdeploy input plugin, after its ELF rewriting pass. The loader
    # and glibc must remain unmodified and come from the same build system.
    cache = subprocess.check_output(["ldconfig", "-p"], text=True)
    architecture = platform.machine()
    libc = find_library("libc.so.6", cache, architecture)
    interpreter = Path(
        subprocess.check_output(
            ["patchelf", "--print-interpreter", payload / "usr/bin/xiaozhu-xiuxiu"], text=True
        ).strip()
    )
    destination = payload / "usr/lib/glibc"
    destination.mkdir(parents=True, exist_ok=True)
    shutil.copy2(interpreter, destination / interpreter.name)
    (destination / "ld-linux.so.2").symlink_to(interpreter.name)
    for name in (
        "libc.so.6",
        "libm.so.6",
        "libdl.so.2",
        "libpthread.so.0",
        "librt.so.1",
        "libresolv.so.2",
        "libutil.so.1",
        "libanl.so.1",
    ):
        shutil.copy2(find_library(name, cache, architecture), destination / name)
    # Older glibc releases use separate modules for local users and DNS.
    for name in ("libnss_files.so.2", "libnss_dns.so.2"):
        if re.search(rf"^\s*{re.escape(name)} ", cache, re.MULTILINE):
            shutil.copy2(find_library(name, cache, architecture), destination / name)
    (destination / "version").write_text(
        os.confstr("CS_GNU_LIBC_VERSION").split()[1] + "\n"
    )
    (destination / "system-loader").write_text(str(interpreter) + "\n")

    notices = payload / "usr/share/licenses/glibc"
    notices.mkdir(parents=True, exist_ok=True)
    if Path("/etc/debian_version").exists():
        shutil.copy2("/usr/share/doc/libc6/copyright", notices / "copyright")
    else:
        files = subprocess.check_output(
            ["rpm", "-qL", "-f", libc], text=True
        ).splitlines()
        for name in files:
            path = Path(name)
            if path.is_file():
                shutil.copy2(path, notices / path.name)
    if not any(notices.iterdir()):
        raise ValueError("Missing glibc license notices")
    for path in (*destination.iterdir(), *notices.iterdir()):
        path.chmod(0o755 if path.name.startswith("ld-") else 0o644)


def build_appimage(payload, output, version, architecture):
    # These libraries are opened with dlopen, so ELF dependency scanning misses them.
    # Keep the Vulkan loader and GPU drivers on the host.
    cache = subprocess.check_output(["ldconfig", "-p"], text=True)
    libraries = []
    for name in [*RPM_LIBRARIES, "libgcc_s.so.1"]:
        if name == "libvulkan.so.1":
            continue
        libraries.extend(("--library", str(find_library(name, cache, architecture))))
    environment = {
        **os.environ,
        "ARCH": architecture,
        "LINUXDEPLOY_OUTPUT_VERSION": version,
        "APPIMAGE_EXTRACT_AND_RUN": "1",
        # linuxdeploy's bundled patchelf is too old for some modern ELF layouts.
        "PATCHELF": shutil.which("patchelf"),
        "NO_STRIP": "1",  # The staged executable is already stripped.
        "LDAI_OUTPUT": str(output),
        "LDAI_NO_APPSTREAM": "1",
        "PATH": f"{ROOT / 'scripts'}{os.pathsep}{os.environ['PATH']}",
    }
    subprocess.run(
        [
            "linuxdeploy",
            "--appdir",
            payload,
            "--custom-apprun",
            ROOT / "packaging/AppRun",
            "--desktop-file",
            payload / "usr/share/applications/com.xiaozhu.xiuxiu.desktop",
            "--icon-file",
            payload / "usr/share/icons/hicolor/256x256/apps/com.xiaozhu.xiuxiu.png",
            *libraries,
        ],
        env=environment,
        check=True,
    )
    copy_rpm_library_licenses(payload, cache)
    subprocess.run(
        [
            "linuxdeploy",
            "--appdir",
            payload,
            "--plugin",
            "xuan",
            "--output",
            "appimage",
        ],
        env=environment,
        check=True,
    )
    output.chmod(0o755)


def main():
    os.umask(0o022)
    parser = argparse.ArgumentParser(description=__doc__)
    formats = ("deb", "rpm", "appimage", "all")
    parser.add_argument("--check-tools", choices=formats)
    parser.add_argument("--bundle-glibc", type=Path)
    parser.add_argument("format", nargs="?", choices=formats)
    parser.add_argument("stage", nargs="?", type=Path)
    parser.add_argument("version", nargs="?")
    parser.add_argument("output", nargs="?", type=Path)
    args = parser.parse_args()
    if args.bundle_glibc:
        bundle_glibc(args.bundle_glibc)
        return
    if args.check_tools:
        check_tools(args.check_tools)
        return
    if not all((args.format, args.stage, args.version, args.output)):
        parser.error("format, stage, version, and output are required")
    check_tools(args.format)
    version = native_version(args.version)
    architecture = platform.machine()
    glibc = glibc_requirement(args.stage / "bin/xiaozhu-xiuxiu", architecture)
    args.output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".native-", dir=args.output) as directory:
        temporary = Path(directory).resolve()
        payload = temporary / "payload"
        stage_payload(args.stage, payload)
        formats = ("deb", "rpm", "appimage") if args.format == "all" else (args.format,)
        for package_format in formats:
            extension = "AppImage" if package_format == "appimage" else package_format
            name = f"xiaozhu-xiuxiu-{args.version}"
            if package_format != "appimage":
                name += "-linux"
            output = args.output.resolve() / f"{name}-{architecture}.{extension}"
            if package_format == "deb":
                build_deb(payload, output, version, architecture, glibc)
            elif package_format == "rpm":
                build_rpm(payload, output, version, architecture, temporary)
            else:
                build_appimage(payload, output, args.version, architecture)
            with output.open("rb") as stream:
                digest = hashlib.file_digest(stream, "sha256").hexdigest()
            output.with_name(output.name + ".sha256").write_text(
                f"{digest}  {output.name}\n"
            )
            print(f"Created {output}")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, subprocess.CalledProcessError) as error:
        raise SystemExit(str(error)) from error
