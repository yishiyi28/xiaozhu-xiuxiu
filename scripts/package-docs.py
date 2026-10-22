#!/usr/bin/env python3
"""Stage user documentation with links appropriate for installed packages."""

import argparse
import re
from pathlib import Path
from urllib.parse import quote, urlsplit

ROOT = Path(__file__).resolve().parent.parent
DOCUMENTS = {
    "packaging/README.md": "README.md",
    "docs/USAGE.md": "USAGE.md",
    "docs/SHORTCUTS.md": "SHORTCUTS.md",
    "docs/RAW.md": "RAW.md",
    "THIRD_PARTY.md": "THIRD_PARTY.md",
}
LOCAL_LINKS = {
    **DOCUMENTS,
    "packaging/SOURCES.md": "SOURCES.md",
    "LICENSE": "../../licenses/xiaozhu-xiuxiu/LICENSE",
    "licenses/rawler-LGPL-2.1.txt": "../../licenses/xiaozhu-xiuxiu/rawler-LGPL-2.1.txt",
    "licenses/heic-rs-MIT.txt": "../../licenses/xiaozhu-xiuxiu/heic-rs-MIT.txt",
    "licenses/tabler-icons-MIT.txt": "../../licenses/xiaozhu-xiuxiu/tabler-icons-MIT.txt",
    "assets/fonts/Inter-LICENSE.txt": "../../licenses/xiaozhu-xiuxiu/Inter-LICENSE.txt",
    "assets/fonts/DroidSansFallback-LICENSE.txt": "../../licenses/xiaozhu-xiuxiu/DroidSansFallback-LICENSE.txt",
    "vendor/egui-winit/LICENSE-MIT": "../../licenses/xiaozhu-xiuxiu/egui-winit/LICENSE-MIT",
    "vendor/egui-winit/LICENSE-APACHE": "../../licenses/xiaozhu-xiuxiu/egui-winit/LICENSE-APACHE",
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path)
    parser.add_argument("version")
    args = parser.parse_args()
    args.destination.mkdir(parents=True)
    repository = "https://github.com/yishiyi28/xiaozhu-xiuxiu"
    tag = quote(f"v{args.version}", safe="")

    for relative, name in DOCUMENTS.items():
        source = ROOT / relative

        def rewrite_link(match, source=source):
            target = urlsplit(match[2])
            if target.scheme or target.netloc or not target.path:
                return match[0]
            path = (source.parent / target.path).resolve().relative_to(ROOT).as_posix()
            destination = LOCAL_LINKS.get(path)
            if destination is None:
                destination = f"{repository}/blob/{tag}/{quote(path)}"
            if target.fragment:
                destination += f"#{target.fragment}"
            return f"{match[1]}{destination})"

        content = re.sub(
            r"(\[[^\]]*\]\()([^)]+)\)", rewrite_link, source.read_text(encoding="utf-8")
        )
        (args.destination / name).write_text(content, encoding="utf-8", newline="\n")

    archive = f"xiaozhu-xiuxiu-{args.version}-source.tar.gz"
    (args.destination / "SOURCES.md").write_text(
        f"# Sources for 小猪修修 {args.version}\n\n"
        f"Download [{archive}]({repository}/releases/download/{tag}/{archive}) "
        f"and its [.sha256 file]({repository}/releases/download/{tag}/{archive}.sha256) "
        f"from the [matching release]({repository}/releases/tag/{tag}).\n\n"
        "This archive contains the matching application sources, patched egui-winit, "
        "and the exact LGPL-licensed Rawler sources used in the binary. "
        "It is provided alongside all binary downloads; retain it when redistributing them. "
        "GitHub's automatic source archives do not contain the vendored Rawler sources.\n\n"
        "To rebuild or relink with a modified Rawler, extract the archive into a writable "
        f"directory, enter `xiaozhu-xiuxiu-{args.version}-source`, edit `vendor/rawler` if desired, "
        "and run `cargo build --release --locked`. Build prerequisites and further "
        "instructions are in `docs/DEVELOPMENT.md` inside the source archive.\n",
        encoding="utf-8",
        newline="\n",
    )
    (args.destination / "copyright").write_text(
        (ROOT / "LICENSE").read_text(encoding="utf-8")
        + "\nDependency licenses are under share/licenses/xiaozhu-xiuxiu in this installation.\n\n"
        + (args.destination / "THIRD_PARTY.md").read_text(encoding="utf-8"),
        encoding="utf-8",
        newline="\n",
    )


if __name__ == "__main__":
    main()
