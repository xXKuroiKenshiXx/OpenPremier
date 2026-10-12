"""Retain omitted workspace licenses from exact Cargo-recorded Git revisions.

Only public HTTPS GitHub raw text is fetched. No downloaded code is executed.
Usage: python tools/collect-upstream-notices.py <cargo-metadata.json> [...]
"""
import concurrent.futures
import hashlib
import json
import re
import sys
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "licenses" / "upstream"
NAMES = ("LICENSE", "LICENSE.txt", "LICENSE.md", "LICENSE-MIT", "LICENSE-APACHE", "LICENSE-ZLIB", "COPYING", "COPYING.MIT", "COPYING.APACHE2", "UNLICENSE", "license.txt")
# This published crate omits repository metadata. Its Cargo VCS subdirectory,
# upstream workspace and original source identify it as part of fframes.
KNOWN_REPOSITORIES = {name: "https://github.com/dmtrKovalenko/fframes" for name in
                      ("fframes", "fframes_skia_renderer", "fframes-media", "fframes-media-dir-macro", "svgr-macro")}


def missing_packages(metadata_paths):
    packages = {}
    for path in metadata_paths:
        for package in json.loads(Path(path).read_text(encoding="utf-8-sig"))["packages"]:
            if not package.get("source"):
                continue
            folder = Path(package["manifest_path"]).parent
            if any(p.is_file() and ("license" in p.name.lower() or "licence" in p.name.lower() or p.name.upper().startswith(("COPYING", "NOTICE"))) and p.suffix.lower() not in (".a", ".lib", ".dll", ".exe") for p in folder.rglob("*")):
                continue
            info_path = folder / ".cargo_vcs_info.json"
            repo = urllib.parse.urlsplit(package.get("repository") or KNOWN_REPOSITORIES.get(package["name"], ""))
            if repo.hostname != "github.com" or not info_path.is_file():
                continue
            info = json.loads(info_path.read_text())
            revision = info.get("git", {}).get("sha1", "")
            repo_path = repo.path.strip("/").removesuffix(".git").split("/")
            if len(repo_path) < 2 or not re.fullmatch(r"[0-9a-f]{40}", revision):
                continue
            repository = "/".join(repo_path[:2])
            key = f'{package["name"]}-{package["version"]}'
            if not re.fullmatch(r"[A-Za-z0-9_.+\-]+", key):
                raise ValueError("Invalid crate directory")
            packages[key] = {"name": package["name"], "version": package["version"], "repository": repository, "revision": revision}
    return packages


def fetch_repository(item):
    repository, revision = item
    found = []
    for name in NAMES:
        url = f"https://raw.githubusercontent.com/{repository}/{revision}/{name}"
        try:
            with urllib.request.urlopen(url, timeout=12) as response:
                data = response.read(2_000_001)
            if len(data) > 2_000_000:
                raise ValueError("License exceeds text limit")
            data.decode("utf-8")
            found.append((name, url, data))
        except (urllib.error.URLError, TimeoutError, UnicodeDecodeError):
            continue
    return item, found


def main():
    packages = missing_packages(sys.argv[1:])
    items = sorted({(p["repository"], p["revision"]) for p in packages.values()})
    with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
        fetched = dict(pool.map(fetch_repository, items))
    OUTPUT.mkdir(parents=True, exist_ok=True)
    index = []
    for key, package in sorted(packages.items()):
        files = []
        for name, url, data in fetched[(package["repository"], package["revision"])]:
            folder = OUTPUT / key
            folder.mkdir(exist_ok=True)
            path = folder / name
            if path.exists() and path.read_bytes() != data:
                raise ValueError(f"Refusing to overwrite changed attribution: {path}")
            path.write_bytes(data)
            files.append({"path": str(path.relative_to(ROOT)).replace("\\", "/"), "url": url, "sha256": hashlib.sha256(data).hexdigest()})
        index.append({**package, "files": files})
    (OUTPUT / "index.json").write_text(json.dumps({"packages": index}, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"crates": len(index), "resolved": sum(bool(p["files"]) for p in index), "unresolved": [p["name"] + " " + p["version"] for p in index if not p["files"]]}))


if __name__ == "__main__":
    main()
