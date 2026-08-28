# Supply-chain gate

A standing check that a malicious or unreviewed crate change fails our build
instead of being noticed on YouTube.

It exists because of the crates.io attack of **2026-08-20**: the popular
`arrayref` crate shipped a malicious update that added a build script pulling a
typosquatted `proc-macro-1` (a look-alike of `proc-macro2`). The payload ran on
`cargo build`, before any of the crate's own code was invoked — it disabled TLS
verification, fetched a platform payload, enumerated browser databases, and set
up persistence. There was **no advisory for it on day one**, because it was a
brand-new release.

So this kit has two halves. One catches *known* bad crates (`cargo audit`). The
other catches the *shape* of that attack before any advisory exists — a
dependency that suddenly grows a build script, a build script that gains the
ability to run a process or open the network, or a dependency named one keystroke
away from a popular crate.

## The three gates

| Gate | Tool | Catches | Does **not** catch |
|---|---|---|---|
| Known vulnerabilities | `cargo audit` (`audit.sh`) | Any crate/version with a RustSec advisory | A brand-new malicious release with no advisory yet |
| Build-script drift + typosquats | `guard.py` (ours, stdlib-only) | A crate that **gains** a build script; a build script whose content changes at a fixed version; a build script that gains process/network capability; a dependency name that is a near-miss of a popular crate | A malicious crate whose build script looked benign at baseline and stays byte-identical; a payload hidden in *runtime* code rather than the build script |
| Standing policy | `cargo deny` (`deny.toml`) | A dependency from a **git/path/unknown registry** (not crates.io); a crate with a **non-allow-listed licence**; duplicate versions (reported) | Anything about the *contents* of an allowed-source, allowed-licence crate |

Read across the row: no single gate is sufficient, which is the point of running
all three. The honest limits are in the "does not catch" column and in
[Would this have caught 20 August?](#would-this-have-caught-20-august) below.

## Fast vs thorough

- **`check.sh`** — the fast gate. Only `guard.py`: offline, no advisory fetch, no
  `cargo metadata` resolve. **~0.09s** on this repo. Safe to run on every build /
  commit / push. This is the half that would have fired on 20 August.
- **`scan.sh`** — the thorough scan. All three gates. **~2–7s** (the range is the
  `cargo audit` advisory-DB fetch). Run it on a schedule or before a release.

Install the fast gate as a pre-push hook:

```sh
bash tools/supplychain/install-hooks.sh   # once per clone
```

Run the thorough scan any time:

```sh
tools/supplychain/scan.sh
```

## The build-script baseline

`guard.py --update-baseline` writes two committed files:

- `buildscript-baseline.json` — the machine baseline the gate compares against.
  For **every** locked crate it records name, version and whether it ships a build
  script; for each build script it records the file, a SHA-256 of its bytes, and
  the capabilities it was reviewed to have (process execution / network).
- `buildscript-inventory.md` — the human-readable companion. Review it in a diff.

The baseline is the reviewed, trusted state. The gate fails when the tree drifts
from it. When a change is **legitimate and reviewed**, re-baseline:

```sh
python3 tools/supplychain/guard.py --update-baseline
```

That is the intended workflow, not a way to silence the gate: a version bump of a
build-script crate, or a new build-script dependency, *should* stop the build and
make a human look at the diff before re-baselining.

## "Cannot run" is a failure, not a pass

`cargo audit` returns exit **1** for *both* "a vulnerability was found" *and* "the
advisory database could not be loaded". A naive `cargo audit || …` cannot tell
them apart, and the classic mistake — `cargo audit || true`, added so unmaintained
warnings stop breaking CI — turns a failed advisory fetch into a silent green.

`audit.sh` never does that. It classifies the outcome from the output, not the
exit code alone:

- **0** — scan ran, no vulnerabilities (warnings are reported, not fatal)
- **2** — scan ran, vulnerabilities found
- **3** — scan **could not run** (advisory DB unreachable *and* no cached copy)

Exit 3 is loud and distinct. If the live fetch fails but a cached DB exists, the
scan still runs against the cache and warns — loudly, with the DB's age — that
results may be stale. Only with no DB at all does it fail with 3.

## Proof the gates fire

A gate nobody has seen fail is not a gate. Both self-tests build throwaway
fixtures (a temp `CARGO_HOME` / temp repos — the real `~/.cargo` and this repo are
never touched) and show each gate go red on the thing it targets, then green when
reverted:

```sh
tools/supplychain/selftest.sh        # guard.py: 6 scenarios incl. the arrayref shape
tools/supplychain/deny-selftest.sh   # cargo deny: licences, sources, bans, duplicates, clean
```

`selftest.sh` reproduces the 20 August attack shape directly: `arrayref` gains a
build script that runs a process and opens a socket → **GAINED BUILD SCRIPT**; a
`proc-macro-1` enters the tree → **TYPOSQUAT SUSPECT** of `proc-macro2`.

## Would this have caught 20 August?

Honestly: **yes, the build-script guard would have — `cargo audit` would not.**

- `cargo audit` would **not** have caught it on day one. There was no advisory.
  This is exactly why the second gate exists.
- `guard.py` **would** have. The attack's mechanism was `arrayref`, which ships no
  build script, suddenly shipping one — the precise thing the "gained a build
  script" check fails on, before any advisory. The malicious build script also ran
  a process and opened the network, which the capability check flags on top.
- The typosquat check is a **second, weaker** signal here. `proc-macro-1` vs
  `proc-macro2` is Levenshtein distance **2**, not 1, so the strict edit-distance-1
  rule alone would miss it; the guard also runs a punctuation/digit-normalized
  "confusable" comparison (`proc-macro-1` and `proc-macro2` both normalize to
  `procmacro`) which **does** flag it. Either way, the build-script gate is the one
  that fires first and hardest.

What would still get past all three: a crate that ships a build script *at
baseline time* which looks benign and never changes bytes, or a payload placed in
ordinary runtime code rather than a build script. Those need code review or
runtime sandboxing, which this kit does not attempt.

## Porting to another Cargo repo

Every script here is generic — no per-repo edits. Only two things are per-repo,
and `bootstrap.sh` produces/points at both:

```sh
cp -r <showreel>/tools/supplychain  <target>/tools/
cp    <showreel>/deny.toml          <target>/
cd <target> && bash tools/supplychain/bootstrap.sh
```

`bootstrap.sh` generates the target repo's own `buildscript-baseline.json`, runs
both self-tests, and tells you which licences (if any) to add to `deny.toml`'s
allow-list for that tree. The two per-repo tuning points in `deny.toml` are marked
in the file: the licence `allow`/`exceptions` list, and any `[advisories] ignore`
entries.

## Files

| File | Role |
|---|---|
| `guard.py` | Build-script drift + typosquat checker (stdlib only) |
| `buildscript-baseline.json` | Committed machine baseline (generated) |
| `buildscript-inventory.md` | Committed human-readable inventory (generated) |
| `audit.sh` | `cargo audit` wrapper with loud offline handling |
| `check.sh` | Fast gate — `guard.py` only |
| `scan.sh` | Thorough scan — all three gates |
| `install-hooks.sh` | Installs the pre-push hook |
| `bootstrap.sh` | One-time setup when dropping the kit into a repo |
| `selftest.sh` | Proof harness for `guard.py` |
| `deny-selftest.sh` | Proof harness for `cargo deny` |
| `../../deny.toml` | `cargo deny` policy (repo root) |

Requires: `python3` (stdlib only), and for the thorough scan
`cargo install cargo-audit cargo-deny --locked`.
