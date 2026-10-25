# Ruvane CLI

Alpha.8 supplies a `ruvane` binary in the same package as the library, with
no third-party dependencies. It requires a Rust/Cargo toolchain. Generated
applications depend only on the first-party Ruvane library.

## Install and create

```sh
cargo install --path . --locked
ruvane new mon-site --framework-path .
cd mon-site
ruvane dev
```

The first two commands above run from a framework checkout. `--framework-path`
records a validated absolute local dependency path in Cargo.toml. Without that
option, generation pins the CLI's exact Ruvane version from crates.io; it must
be available there. Generation itself performs no network request. Cargo may
resolve dependencies when compiling; `--offline` is available on dev/build.

`new` accepts a lowercase ASCII project identifier beginning with a letter,
at most 48 bytes, using letters, digits, hyphens and underscores. Reserved
device/framework names and Rust keywords are refused. The destination must be
new. Framework paths are escaped as TOML strings; controls are rejected. Files
are created exclusively, with no overwrite or recursive cleanup on failure.

The generated layout contains Cargo.toml, an independent Cargo workspace,
`Ruvane.project`, README, .gitignore, `src/main.rs`, `src/pages.rs`,
`src/components.rs`, and `assets/site.css`. Edit pages and shared layout in
those source files. Static parameters are enumerated explicitly in `pages::site`.
The fixed binary target is `ruvane-app`. The small `Ruvane.project` marker
declares protocol version 1; it is not an authentication mechanism.

## Development

```sh
ruvane dev --port 3000
ruvane dev --port 0 --offline
```

Run dev/build at the generated project root. The CLI does not search parent
directories for another Cargo project. It builds the fixed application target
through Cargo with separate arguments and an isolated `.ruvane/target` directory.
Each running application uses a private executable copy, so compiling a
replacement does not replace its active executable in place.

Dev polls content fingerprints, not just timestamps, in `src/`, `assets/`,
Cargo.toml, Cargo.lock, and Ruvane.project. Paths outside that list, including
target, dist, .git and .ruvane, are not watched. A 200 ms quiet period consolidates
changes; polling waits 150 ms between scans. At most one compilation runs.
Edits during compilation are detected on the next scan. Cargo's own lockfile
write is absorbed without causing a redundant rebuild.

Compilation failure leaves the previous server active. After successful
compilation the CLI stops the old server and starts the new executable at the
same port; there can be a short interruption. If startup fails, it attempts to
restart the retained previous executable. Restoration is not a guarantee against
OS errors or a newly occupied port. A crashed app waits for a source change
instead of entering an unlimited restart loop. The browser refresh is manual;
the fixed CSP still blocks scripts.

Type `q` then Enter, or close stdin, for normal shutdown. Generated servers
have a dedicated stdin pipe: data or EOF requests the library's shutdown.
The CLI allows up to two seconds for normal child exit, then uses kill/wait
for that direct child. It removes only known run files and an empty run
directory; unexpected files are retained. The project's lock is released on
normal exit. A hard termination can leave a lock/run directory; inspect it
after confirming no other CLI instance is active before removing stale data.
Control-C uses normal OS signal behavior; this release does not install a
cross-platform signal handler or sandbox arbitrary process descendants.

## Build versions

Stop dev before building; the project lock rejects simultaneous CLI builds.

```sh
ruvane build --origin https://site.example --offline
ruvane preview dist
ruvane preview dist/build-000001 --port 3002
```

`build` requires an explicit valid public origin. `.example` is a placeholder.
Exports go into fresh `dist/build-000001`, `dist/build-000002`, and so on.
The `dist/current` text record selects the newest successfully published build.
Only after the export returns success and the framework validates the complete
manifest/files does the CLI replace that small record. Compile/export failure
keeps the old selection and old files. This is not a filesystem transaction,
an integrity signature, or a power-loss durability guarantee.

Existing unknown directories and flat alpha.7 exports are not converted or
deleted. Use `--out dist-next` for another simple project-local output name.
Do not use source directories as build outputs. Completed and incomplete build
slots are retained, up to 128 slots per container. Choose another output root
or manage old versions explicitly when reaching that cap. Custom output names
should be added to the project's ignore rules as needed.

The container's `.ruvane-builds` marker and current pointer have strict bounded
formats. Preview supports either a container or a flat completed export. For
the library API, `StaticSite::preview_export` still takes an actual flat build
directory; container selection is a CLI function. Hosting must serve the
selected version's content root and apply its generated header contract.
See [static export](static-export.md).

## Preview

```sh
ruvane preview dist --port 3001
```

Open the printed HTTP URL, not a local index.html file. Preview loads the
selected actual files once using the library's bounded manifest loader and
serves exact routes with the existing loopback HTTP profile and CSP. It does
not compile the project, change files, or reload an already running snapshot.
The manifest, completion marker, symlinks, MIME declarations and lengths are
validated as in alpha.7. A flat export from that version remains supported.

## Process protocol and limits

The generated `ruvane-app` implements two explicit local operations:

- `--ruvane-serve PORT READY_FILE`: bind loopback, start stdin shutdown handling,
  write `ruvane-ready<TAB>1<TAB>127.0.0.1:PORT<LF>`, then run the HTTP server.
- `--ruvane-export DIRECTORY ORIGIN`: enumerate its static pages and export
  to the supplied fresh directory using `StaticSite`.

The CLI fixes the executable/operation and passes values as process arguments,
never a shell string. The environment's `CARGO` executable, or Cargo on PATH,
is trusted. It requires readiness within five seconds, validates the bounded
loopback address, allows 120 seconds for Cargo and 30 seconds for export, and
kills/reaps its direct child on timeout or normal cancellation. Child output
is inherited rather than collected into an unbounded memory buffer.

| Resource | Limit |
| --- | --- |
| CLI arguments / each argument | 32 / 4096 OS-string bytes |
| Watched files, including manifests/lockfile | 1024 |
| Watched file bytes / aggregate | 16 MiB / 64 MiB |
| Cargo.lock | 1 MiB, also within aggregate budgets |
| Watched directories / depth | 128 / 16 |
| Entries in one watched directory | 1024 |
| Retained private run directories | fewer than 64 before creating another |
| Private executable copy | 64 MiB |
| Versioned build slots | 128 per output container |

Fingerprints are non-cryptographic change detectors, not integrity checks.
The original static file/count/total budgets still constrain valid exports.

## Trust and verification

Local project source, Cargo configuration, build scripts, toolchain binaries,
and application code are trusted executable inputs. No compilation endpoint
is exposed over HTTP. The CLI cannot sandbox arbitrary Rust code or control
all descendants it might spawn. Parent/work/output directories must remain
protected from hostile concurrent changes. Observed symlinks and unknown
managed roots are refused; markers are not proof of ownership against an
attacker with write access. No recursive deletion of user directories occurs.

Tests generate and compile a real local project offline, check its formatting
and Clippy, build two versions, preserve selection on compile/export failure,
exercise dynamic HTTP routes and automatic rebuilds, preserve/restore servers
on compile/startup failure, verify same-port recovery and normal process exit,
and preview the selected on-disk build. Additional tests cover path/argument
injection, TOML escaping, locks, pointer validation, file/depth budgets,
symlinks, and cleanup that retains foreign files. These are regressions, not
an independent audit or exhaustive OS/process/filesystem coverage.
