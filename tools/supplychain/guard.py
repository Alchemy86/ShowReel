#!/usr/bin/env python3
"""Supply-chain drift guard for Cargo projects — the check `cargo audit` cannot do.

`cargo audit` needs an advisory to already exist. The 2026-08-20 crates.io attack
(a malicious `arrayref` release that added a build script pulling a typosquatted
`proc-macro-1`) had no advisory on day one. What *would* have caught it, before any
advisory, is the SHAPE of the change:

  1. a dependency that never had a build script suddenly ships one
  2. a build script gaining the ability to run processes or open the network
  3. a dependency whose name is a near-miss of a much more popular crate

This script builds a committed baseline of every locked crate — which ones ship a
build script, what each one invokes, and a content hash of each script — and then,
on every run, fails loudly if the tree drifts away from that baseline in any of the
three shapes above.

Zero third-party dependencies: standard library only. Reads `Cargo.lock`, locates
each crate's *actual* source (extracted registry cache, else the checksummed
`.crate` tarball), and inspects the real build script bytes — not a guess.

Usage:
    guard.py --update-baseline   # (re)generate the committed baseline after review
    guard.py --check             # compare tree to baseline; exit 2 on drift (the gate)
    guard.py --report            # human summary of the current tree; never fails
    guard.py                     # same as --check

Exit codes:
    0  clean — tree matches baseline, no typosquat suspects
    2  drift detected (a gate fired) — see output
    3  the check could NOT run (no baseline / unreadable sources) — loud, never silent

The exit-3 case matters: a supply-chain gate that silently passes when it cannot
actually inspect the tree is worse than no gate. "Cannot run" is a failure, not a pass.
"""

import argparse
import glob
import hashlib
import json
import os
import re
import sys
import tarfile
import tomllib

HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_BASELINE = os.path.join(HERE, "buildscript-baseline.json")
DEFAULT_INVENTORY_MD = os.path.join(HERE, "buildscript-inventory.md")

# --- capability signatures -------------------------------------------------
# Matched against build-script source with comments stripped first, so a URL or a
# `Command::new` that appears only inside a comment does not count (the audit found
# all 12 URL/command mentions in the current tree to be exactly that — benign).
PROC_PATTERNS = [
    r"\bCommand::new\b",
    r"\bprocess::Command\b",
]
NET_PATTERNS = [
    r"\bTcpStream\b", r"\bTcpListener\b", r"\bUdpSocket\b",
    r"\bstd\s*::\s*net\b", r"::\s*net\s*::",
    r"\breqwest\b", r"\bureq\b", r"\bhyper\b", r"\bcurl\b",
    r"\bsocket2\b", r"\battohttpc\b", r"\bisahc\b", r"\bminreq\b",
    r"https?://",
]

# A curated, static set of high-download crates.io names, so a near-miss can be
# spotted with no network. "Much more popular than C" is encoded as "P is on this
# list and C is not" — an honest proxy for download counts we cannot fetch offline.
# The live tree's own names are added to this set at runtime.
POPULAR_CRATES = {
    "serde", "serde_json", "serde_derive", "serde_core", "syn", "quote",
    "proc-macro2", "libc", "rand", "rand_core", "rand_chacha", "getrandom",
    "tokio", "tokio-util", "tokio-macros", "log", "env_logger", "itoa", "ryu",
    "cfg-if", "bitflags", "regex", "regex-syntax", "regex-automata", "clap",
    "clap_derive", "anyhow", "thiserror", "thiserror-impl", "once_cell",
    "base64", "chrono", "futures", "futures-core", "futures-util", "hashbrown",
    "memchr", "unicode-ident", "unicode-width", "unicode-normalization",
    "autocfg", "num-traits", "num-integer", "num-bigint", "aho-corasick",
    "semver", "toml", "toml_edit", "indexmap", "smallvec", "lazy_static",
    "ppv-lite86", "time", "bytes", "http", "http-body", "hyper", "reqwest",
    "url", "idna", "percent-encoding", "form_urlencoded", "mio", "socket2",
    "tracing", "tracing-core", "tracing-subscriber", "pin-project",
    "pin-project-lite", "pin-utils", "slab", "scopeguard", "parking_lot",
    "parking_lot_core", "lock_api", "crossbeam", "crossbeam-channel",
    "crossbeam-deque", "crossbeam-epoch", "crossbeam-utils", "rayon",
    "rayon-core", "either", "itertools", "flate2", "miniz_oxide", "crc32fast",
    "adler", "adler2", "backtrace", "addr2line", "gimli", "object", "rustc-demangle",
    "libm", "wasm-bindgen", "js-sys", "web-sys", "wasm-bindgen-backend",
    "wasm-bindgen-macro", "console_error_panic_hook", "getopts", "atty",
    "termcolor", "ansi_term", "winapi", "windows-sys", "windows-targets",
    "windows_x86_64_gnu", "core-foundation", "core-foundation-sys",
    "security-framework", "openssl", "openssl-sys", "native-tls", "rustls",
    "ring", "untrusted", "sct", "webpki", "webpki-roots", "spin", "digest",
    "sha2", "sha1", "md-5", "hmac", "block-buffer", "generic-array",
    "typenum", "crypto-common", "subtle", "byteorder", "bytemuck", "num_cpus",
    "dirs", "dirs-sys", "home", "tempfile", "fastrand", "walkdir", "same-file",
    "globset", "ignore", "csv", "csv-core", "encoding_rs", "mime", "httparse",
    "h2", "tower", "tower-service", "tower-layer", "async-trait", "futures-io",
    "futures-sink", "futures-task", "futures-channel", "futures-macro",
    "serde_urlencoded", "uuid", "rustversion", "cc", "pkg-config", "vcpkg",
    "jobserver", "cfg_aliases", "prettyplease", "heck", "strsim", "textwrap",
    "unicode-segmentation", "unicode-xid", "proc-macro-error", "darling",
    "syn_derive", "convert_case", "phf", "phf_shared", "siphasher",
    "ahash", "fnv", "rustc-hash", "twox-hash", "wyhash", "foldhash",
    "zerocopy", "zerocopy-derive", "arrayref", "arrayvec", "tinyvec",
    "image", "png", "jpeg-decoder", "gif", "tiny-skia", "tiny-skia-path",
    "fontdb", "rustybuzz", "ttf-parser", "iana-time-zone", "num-conv",
    "powerfmt", "deranged", "time-core", "time-macros", "libloading",
}

# ---------------------------------------------------------------------------


def find_lockfile(start):
    d = os.path.abspath(start)
    while True:
        cand = os.path.join(d, "Cargo.lock")
        if os.path.isfile(cand):
            return cand
        parent = os.path.dirname(d)
        if parent == d:
            return None
        d = parent


def cargo_home():
    return os.path.expanduser(os.environ.get("CARGO_HOME", "~/.cargo"))


def registry_src_roots():
    return glob.glob(os.path.join(cargo_home(), "registry", "src", "*/"))


def registry_cache_roots():
    return glob.glob(os.path.join(cargo_home(), "registry", "cache", "*/"))


class CrateSource:
    """Read files from a crate's source, whether extracted or still a .crate tarball."""

    def __init__(self, name, ver):
        self.name = name
        self.ver = ver
        self.dir = None
        self.tar = None
        for r in registry_src_roots():
            p = os.path.join(r, f"{name}-{ver}")
            if os.path.isdir(p):
                self.dir = p
                return
        for r in registry_cache_roots():
            p = os.path.join(r, f"{name}-{ver}.crate")
            if os.path.isfile(p):
                self.tar = p
                return

    @property
    def available(self):
        return self.dir is not None or self.tar is not None

    def read(self, relpath):
        """Return file bytes, or None if the file is absent."""
        if self.dir is not None:
            p = os.path.join(self.dir, relpath)
            if os.path.isfile(p):
                with open(p, "rb") as f:
                    return f.read()
            return None
        if self.tar is not None:
            member = f"{self.name}-{self.ver}/{relpath}"
            try:
                with tarfile.open(self.tar, "r:*") as t:
                    fo = t.extractfile(member)
                    return fo.read() if fo else None
            except (KeyError, tarfile.TarError):
                return None
        return None

    def isfile(self, relpath):
        if self.dir is not None:
            return os.path.isfile(os.path.join(self.dir, relpath))
        if self.tar is not None:
            member = f"{self.name}-{self.ver}/{relpath}"
            try:
                with tarfile.open(self.tar, "r:*") as t:
                    return t.getmember(member).isfile()
            except (KeyError, tarfile.TarError):
                return False
        return False


def build_script_path(src):
    """Return the crate's build-script relative path, or None. Mirrors cargo's rule:
    an explicit `build = "<file>"`/`build = false` in Cargo.toml wins; otherwise a
    `build.rs` at the crate root is auto-detected."""
    raw = src.read("Cargo.toml")
    build_field = None
    if raw is not None:
        try:
            ct = tomllib.loads(raw.decode("utf-8", "replace"))
            build_field = ct.get("package", {}).get("build")
        except Exception:
            build_field = None
    if build_field is False:
        return None
    if isinstance(build_field, str):
        return build_field if src.isfile(build_field) else None
    if src.isfile("build.rs"):
        return "build.rs"
    return None


def strip_comments(code):
    code = re.sub(r"/\*.*?\*/", " ", code, flags=re.DOTALL)
    code = re.sub(r"//[^\n]*", " ", code)
    return code


def detect_caps(script_bytes):
    """Return (process_exec, network, snippets) for a build script's bytes."""
    text = script_bytes.decode("utf-8", "replace")
    scan = strip_comments(text)
    snippets = []
    proc = False
    net = False
    for pat in PROC_PATTERNS:
        if re.search(pat, scan):
            proc = True
    for pat in NET_PATTERNS:
        if re.search(pat, scan):
            net = True
    # Collect human-reviewable evidence lines (from the ORIGINAL text so line
    # numbers and context read naturally), keyed on the same anchors.
    ev_pats = PROC_PATTERNS + NET_PATTERNS
    for i, line in enumerate(text.splitlines(), 1):
        stripped_line = strip_comments(line)
        for pat in ev_pats:
            if re.search(pat, stripped_line):
                snippets.append(f"{i}: {line.strip()[:160]}")
                break
    return proc, net, snippets


def levenshtein(a, b, cap=2):
    """Edit distance, early-exit once it exceeds `cap`."""
    if a == b:
        return 0
    la, lb = len(a), len(b)
    if abs(la - lb) > cap:
        return cap + 1
    prev = list(range(lb + 1))
    for i in range(1, la + 1):
        cur = [i] + [0] * lb
        rowmin = cur[0]
        for j in range(1, lb + 1):
            cost = 0 if a[i - 1] == b[j - 1] else 1
            cur[j] = min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + cost)
            rowmin = min(rowmin, cur[j])
        if rowmin > cap:
            return cap + 1
        prev = cur
    return prev[lb]


def normalize_name(n):
    """Collapse the punctuation/digit variations typosquats hide behind:
    proc-macro-1 and proc-macro2 both normalize to 'procmacro'."""
    n = n.lower().replace("-", "").replace("_", "")
    return n.rstrip("0123456789")


def load_lock(lockfile):
    data = tomllib.loads(open(lockfile, "rb").read().decode())
    out = []
    for p in data.get("package", []):
        if "source" not in p:  # workspace / path member (the crate itself)
            continue
        out.append((p["name"], p["version"]))
    return out


def scan_tree(lockfile):
    """Return (inventory, unreadable). inventory is keyed by "name version" (a crate
    can appear at several versions in one lock, and each is a distinct source to
    inspect). Each entry carries name, version, has_build, and — when has_build —
    path/sha256/size/caps/snippets."""
    inv = {}
    unreadable = []
    for name, ver in load_lock(lockfile):
        src = CrateSource(name, ver)
        if not src.available:
            unreadable.append((name, ver))
            continue
        bpath = build_script_path(src)
        entry = {"name": name, "version": ver, "has_build": bpath is not None}
        if bpath is not None:
            b = src.read(bpath)
            if b is None:
                unreadable.append((name, ver))
                continue
            proc, net, snips = detect_caps(b)
            entry.update({
                "build_script": bpath,
                "sha256": hashlib.sha256(b).hexdigest(),
                "size": len(b),
                "caps": {"process_exec": proc, "network": net},
                "invokes": snips,
            })
        inv[f"{name} {ver}"] = entry
    return inv, unreadable


# --- typosquat ------------------------------------------------------------

def typosquat_suspects(inv, max_distance):
    tree_names = {e["name"] for e in inv.values()}
    popular = set(POPULAR_CRATES) | tree_names
    suspects = []
    for c in sorted(tree_names):
        if c in POPULAR_CRATES:
            continue  # C is itself a known-popular crate: not a squat of anything
        cnorm = normalize_name(c)
        for p in sorted(POPULAR_CRATES):
            if p == c:
                continue
            d = levenshtein(c, p, cap=max_distance)
            confusable = cnorm == normalize_name(p) and c != p
            if d <= max_distance or confusable:
                reason = (f"edit-distance {d}" if d <= max_distance
                          else "punctuation/digit confusable")
                suspects.append((c, p, reason))
    return suspects


# --- commands -------------------------------------------------------------

def cmd_update_baseline(args, lockfile):
    inv, unreadable = scan_tree(lockfile)
    if unreadable:
        print("REFUSING to write a baseline from an incompletely-readable tree.", file=sys.stderr)
        print("Could not read sources for:", file=sys.stderr)
        for n, v in unreadable:
            print(f"  {n} {v}", file=sys.stderr)
        print("Run `cargo fetch` and retry.", file=sys.stderr)
        return 3
    baseline = {
        "_comment": "Supply-chain build-script baseline. Regenerate with "
                    "`tools/supplychain/guard.py --update-baseline` AFTER reviewing "
                    "every build script that changed. Reviewed by a human is the point. "
                    "Keys are 'name version'.",
        "crates": {k: {"name": e["name"], "version": e["version"], "has_build": e["has_build"]}
                   for k, e in sorted(inv.items())},
        "build_scripts": {k: {kk: e[kk] for kk in
                              ("name", "version", "build_script", "sha256", "size", "caps", "invokes")}
                          for k, e in sorted(inv.items()) if e["has_build"]},
    }
    with open(args.baseline, "w") as f:
        json.dump(baseline, f, indent=2, sort_keys=True)
        f.write("\n")
    write_inventory_md(baseline, args.inventory_md)
    bs = baseline["build_scripts"]
    print(f"Wrote baseline: {args.baseline}")
    print(f"  {len(baseline['crates'])} crates, {len(bs)} with a build script")
    print(f"  {sum(1 for e in bs.values() if e['caps']['process_exec'])} invoke a process, "
          f"{sum(1 for e in bs.values() if e['caps']['network'])} touch the network")
    print(f"Wrote human-readable inventory: {args.inventory_md}")
    return 0


def write_inventory_md(baseline, path):
    bs = baseline["build_scripts"]
    lines = [
        "# Build-script inventory (supply-chain baseline)",
        "",
        "Generated by `tools/supplychain/guard.py --update-baseline`. This is the",
        "human-readable companion to `buildscript-baseline.json` (the machine baseline",
        "the gate compares against). Review this file when it changes in a diff.",
        "",
        f"- **{len(baseline['crates'])}** locked crates total",
        f"- **{len(bs)}** ship a build script",
        f"- **{sum(1 for e in bs.values() if e['caps']['process_exec'])}** invoke a subprocess "
        "(e.g. `rustc`/`git` for version detection)",
        f"- **{sum(1 for e in bs.values() if e['caps']['network'])}** reference the network",
        "",
        "| crate | version | script | process | network | invokes |",
        "|---|---|---|:-:|:-:|---|",
    ]
    for e in sorted(bs.values(), key=lambda x: (x["name"], x["version"])):
        caps = e["caps"]
        inv = "; ".join(s.split(":", 1)[1].strip() for s in e["invokes"][:3]) or "—"
        inv = inv.replace("|", "\\|")
        lines.append(
            f"| `{e['name']}` | {e['version']} | `{e['build_script']}` | "
            f"{'yes' if caps['process_exec'] else '—'} | "
            f"{'yes' if caps['network'] else '—'} | {inv} |"
        )
    lines.append("")
    with open(path, "w") as f:
        f.write("\n".join(lines))


def cmd_check(args, lockfile):
    if not os.path.isfile(args.baseline):
        print("SUPPLY-CHAIN GATE COULD NOT RUN: no baseline at", args.baseline, file=sys.stderr)
        print("Generate one with `guard.py --update-baseline` (after reviewing the tree).",
              file=sys.stderr)
        return 3
    baseline = json.loads(open(args.baseline).read())
    inv, unreadable = scan_tree(lockfile)
    if unreadable:
        print("SUPPLY-CHAIN GATE COULD NOT RUN: source unreadable for these crates:", file=sys.stderr)
        for n, v in unreadable:
            print(f"  {n} {v}", file=sys.stderr)
        print("The gate refuses to report GREEN on a tree it cannot fully inspect.", file=sys.stderr)
        print("Run `cargo fetch` and retry.", file=sys.stderr)
        return 3

    base_crates = baseline.get("crates", {})
    base_bs = baseline.get("build_scripts", {})
    # name-level rollups from the baseline, so "this NAME never had a build script"
    # (the arrayref-gained-a-build-script case) is answerable across version bumps.
    base_names = {c["name"] for c in base_crates.values()}
    base_names_with_build = {c["name"] for c in base_bs.values()}
    # name-level capability rollup: what could each crate NAME do at baseline, across
    # every baselined version of it. Lets a version bump report *capability escalation*
    # (0.3.9 only probed rustc; 0.3.10 opens a socket) rather than a bare "re-review".
    base_caps_by_name = {}
    for c in base_bs.values():
        acc = base_caps_by_name.setdefault(c["name"], {"process_exec": False, "network": False})
        acc["process_exec"] |= c["caps"]["process_exec"]
        acc["network"] |= c["caps"]["network"]
    red = []       # hard failures
    notes = []     # informational drift (not a failure)

    def escalation_clause(name, cur_caps):
        prev = base_caps_by_name.get(name, {"process_exec": False, "network": False})
        esc = []
        if cur_caps["process_exec"] and not prev["process_exec"]:
            esc.append("process execution")
        if cur_caps["network"] and not prev["network"]:
            esc.append("network access")
        if esc:
            return (f" CAPABILITY ESCALATION: it gained {' and '.join(esc)} that no baselined "
                    f"version of `{name}` had.")
        return ""

    for key, e in sorted(inv.items()):
        name = e["name"]
        if not e["has_build"]:
            continue
        cur_caps = e["caps"]
        if key not in base_bs:
            # a (name,version) shipping a build script that the baseline did not bless
            if name in base_names and name not in base_names_with_build:
                red.append(f"GAINED BUILD SCRIPT: `{name}` {e['version']} — this crate had NO "
                           f"build script in the baseline and now ships `{e['build_script']}` "
                           f"(process={cur_caps['process_exec']}, network={cur_caps['network']}). "
                           f"This is the exact shape of the 2026-08-20 arrayref attack.")
            elif name in base_names_with_build:
                red.append(f"BUILD-SCRIPT CRATE VERSION CHANGED: `{name}` gained version "
                           f"{e['version']} carrying `{e['build_script']}` not in the baseline "
                           f"(process={cur_caps['process_exec']}, network={cur_caps['network']}). "
                           f"A version bump means the build script must be re-reviewed; "
                           f"re-baseline after review." + escalation_clause(name, cur_caps))
            else:
                red.append(f"NEW BUILD-SCRIPT CRATE: `{name}` {e['version']} entered the tree "
                           f"already carrying `{e['build_script']}` "
                           f"(process={cur_caps['process_exec']}, network={cur_caps['network']}). "
                           f"Review it, then re-baseline if intended.")
            continue
        b = base_bs[key]
        if b["sha256"] != e["sha256"]:
            red.append(f"BUILD-SCRIPT CONTENT CHANGED AT SAME VERSION: `{name}` {e['version']} "
                       f"— published versions are immutable, so a hash change here means the "
                       f"source was altered. baseline={b['sha256'][:12]} now={e['sha256'][:12]}."
                       + escalation_clause(name, cur_caps))

    # build-script crates that disappeared, and brand-new plain deps: informational only
    cur_names_with_build = {e["name"] for e in inv.values() if e["has_build"]}
    for nm in sorted(base_names_with_build - cur_names_with_build):
        notes.append(f"{nm}: had a build script in the baseline, none in the tree now "
                     f"(dependency changed/removed) — re-baseline when expected")
    for key, e in sorted(inv.items()):
        if key not in base_crates and not e["has_build"]:
            notes.append(f"{e['name']} {e['version']}: new dependency (no build script)")

    suspects = typosquat_suspects(inv, args.max_distance)

    print("== Supply-chain drift guard ==")
    print(f"crates scanned: {len(inv)}   build scripts: {sum(1 for e in inv.values() if e['has_build'])}")
    if notes:
        print(f"\n-- informational drift ({len(notes)}) --")
        for n in notes:
            print("  •", n)
    if suspects:
        print(f"\n-- TYPOSQUAT SUSPECTS ({len(suspects)}) --")
        for c, p, reason in suspects:
            print(f"  ✗ `{c}` resembles popular crate `{p}` ({reason})")
    if red:
        print(f"\n-- BUILD-SCRIPT DRIFT — {len(red)} FAILURE(S) --")
        for r in red:
            print("  ✗", r)

    failed = bool(red) or bool(suspects)
    print()
    if failed:
        print("RESULT: RED — supply-chain drift detected. Review each item above.")
        print("If a change is legitimate and reviewed, re-run `guard.py --update-baseline`.")
        return 2
    print("RESULT: GREEN — tree matches the reviewed baseline; no typosquat suspects.")
    return 0


def cmd_report(args, lockfile):
    inv, unreadable = scan_tree(lockfile)
    bs = {n: e for n, e in inv.items() if e["has_build"]}
    print(f"{len(inv)} crates; {len(bs)} ship a build script")
    if unreadable:
        print(f"  ({len(unreadable)} sources unreadable: {unreadable})")
    for e in sorted(bs.values(), key=lambda x: (x["name"], x["version"])):
        c = e["caps"]
        tag = []
        if c["process_exec"]:
            tag.append("process")
        if c["network"]:
            tag.append("network")
        print(f"  {e['name']} {e['version']}  [{','.join(tag) or 'no-caps'}]  {e['build_script']}")
        for s in e["invokes"][:4]:
            print(f"      {s}")
    return 0


def main():
    ap = argparse.ArgumentParser(description="Cargo supply-chain drift guard (stdlib only).")
    g = ap.add_mutually_exclusive_group()
    g.add_argument("--update-baseline", action="store_true",
                   help="regenerate the committed baseline from the current tree")
    g.add_argument("--check", action="store_true",
                   help="compare tree to baseline; exit 2 on drift (default)")
    g.add_argument("--report", action="store_true",
                   help="print the current build-script inventory; never fails")
    ap.add_argument("--baseline", default=DEFAULT_BASELINE,
                    help="path to the baseline JSON (default: beside this script)")
    ap.add_argument("--inventory-md", default=DEFAULT_INVENTORY_MD,
                    help="path to the human-readable inventory markdown")
    ap.add_argument("--lockfile", default=None, help="path to Cargo.lock (default: search upward)")
    ap.add_argument("--max-distance", type=int, default=1,
                    help="typosquat edit-distance threshold (default: 1)")
    args = ap.parse_args()

    lockfile = args.lockfile or find_lockfile(os.getcwd())
    if not lockfile:
        print("Could not find Cargo.lock from", os.getcwd(), file=sys.stderr)
        return 3

    if args.update_baseline:
        return cmd_update_baseline(args, lockfile)
    if args.report:
        return cmd_report(args, lockfile)
    return cmd_check(args, lockfile)


if __name__ == "__main__":
    sys.exit(main())
