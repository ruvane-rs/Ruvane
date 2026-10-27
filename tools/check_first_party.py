"""Check that the framework and Desk use only local first-party packages."""
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
    assert package["version"] == "0.1.0-alpha.62", "workspace versions must match"
desk = root / "apps/desk"
pilot = json.loads(subprocess.run(
    [os.environ.get("CARGO", "cargo"), "metadata", "--offline", "--locked", "--all-features", "--format-version", "1"],
    cwd=desk, check=True, stdout=subprocess.PIPE, text=True,
).stdout)
pilot_expected = {**expected, "desk": desk / "Cargo.toml", "ruvane-application-browser": desk / "browser/Cargo.toml"}
assert len(pilot["packages"]) == 4 and len(pilot["workspace_members"]) == 2, "unexpected Desk graph"
for package in pilot["packages"]:
    assert package["source"] is None, "third-party dependency is forbidden"
    assert package["name"] in pilot_expected and Path(package["manifest_path"]).resolve() == pilot_expected[package["name"]], "unexpected Desk package path"
    assert package["version"] == "0.1.0-alpha.62", "Desk and framework versions must match"
for source in [*(desk / "src").rglob("*.rs"), *(desk / "browser/src").rglob("*.rs")]:
    assert not re.search(r"\bunsafe\s*\{|\bunsafe\s+(?:fn|extern)|#\[unsafe\(", source.read_text()), "unsafe Desk code is forbidden"
abi = root / "browser/src/abi.rs"
for source in (root / "browser/src").rglob("*.rs"):
    text = source.read_text()
    assert not re.search(r"\bunsafe\s*\{|\bunsafe\s+(?:fn|extern)", text), "unsafe block/function is forbidden"
    for attribute in re.findall(r"#\[unsafe\([^\n]*", text):
        assert source == abi and attribute == "#[unsafe(no_mangle)]", "unexpected unsafe attribute"
symbols = re.findall(r'pub extern "C" fn (\w+)', abi.read_text())
assert len(symbols) == len(set(symbols)) == 22 and all(s.startswith("ruvane_") for s in symbols), "invalid ABI symbols"
print("First-party Cargo packages only; ABI boundary contains reviewed export attributes only.")
