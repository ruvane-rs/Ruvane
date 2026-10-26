"""Check that Cargo uses only our two local packages."""
import json
import os
from pathlib import Path
import re
import subprocess

root = Path(__file__).resolve().parent.parent
result = subprocess.run(
    [os.environ.get("CARGO", "cargo"), "metadata", "--offline", "--locked", "--all-features", "--format-version", "1"],
    cwd=root, check=True, stdout=subprocess.PIPE, text=True,
)
metadata = json.loads(result.stdout)
expected = {"ruvane": root / "Cargo.toml", "ruvane-browser": root / "browser/Cargo.toml"}
assert len(metadata["packages"]) == len(expected), "unexpected Cargo package"
assert len(metadata["workspace_members"]) == len(expected), "unexpected workspace member"
for package in metadata["packages"]:
    assert package["source"] is None, "registry/git dependency is forbidden"
    assert package["name"] in expected and Path(package["manifest_path"]).resolve() == expected[package["name"]], "unexpected package path"
    assert package["version"] == "0.1.0-alpha.28", "workspace versions must match"
abi = root / "browser/src/abi.rs"
for source in (root / "browser/src").rglob("*.rs"):
    text = source.read_text()
    assert not re.search(r"\bunsafe\s*\{|\bunsafe\s+(?:fn|extern)", text), "unsafe block/function is forbidden"
    for attribute in re.findall(r"#\[unsafe\([^\n]*", text):
        assert source == abi and attribute == "#[unsafe(no_mangle)]", "unexpected unsafe attribute"
symbols = re.findall(r'pub extern "C" fn (\w+)', abi.read_text())
assert len(symbols) == len(set(symbols)) == 22 and all(s.startswith("ruvane_") for s in symbols), "invalid ABI symbols"
print("First-party Cargo packages only; ABI boundary contains reviewed export attributes only.")
