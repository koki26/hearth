"""A GDScript extractor for graphify.

    python client/godot/tools/gdscript_graph.py <root> --out ast.json
    python client/godot/tools/gdscript_graph.py --self-check

graphify ships hand-written extractors for thirty-odd languages and GDScript is
not one of them, so a Godot project reads to it as a folder of unknown files:
run it on `client/` and the entire game -- every script in `client/godot` -- is
simply missing from the graph. Its `LANGUAGE_EXTRACTORS` registry looks like the
place to add one, but its own docstring says dispatch does not flow through it
yet, so registering there would do nothing.

So this lives here instead: our repo, versioned with the code it reads, and it
cannot be undone by a `graphify` upgrade. It emits the same node and edge shape
graphify's own extractors do, which the build step merges as the AST tier.

**IDs come from `graphify.ids.make_id` when graphify is importable.** Three
producers have to agree on node IDs -- the AST extractors, the LLM subagents,
and the graph builder -- and when they disagree a single symbol silently
becomes two disconnected ghosts. The fallback below mirrors the documented
recipe for when graphify is not installed, but the real thing is preferred
precisely so the recipe cannot drift.

What it does not do: `.tscn` scenes (they wire nodes to scripts, which would be
a useful second pass), `await`/signal connection graphs beyond the declaration,
and anything inside a string.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import unicodedata
from pathlib import Path

try:  # The real thing, so IDs cannot drift from the other extractors.
    from graphify.ids import make_id as _graphify_make_id
except Exception:  # pragma: no cover - only when graphify is not installed
    _graphify_make_id = None


def normalize_id(s: str) -> str:
    """Mirror of graphify.ids.normalize_id, for when graphify is absent."""
    cur = s
    for _ in range(6):
        nxt = unicodedata.normalize("NFKC", cur.casefold())
        if nxt == cur:
            break
        cur = nxt
    cur = re.sub(r"[^\w]+", "_", cur, flags=re.UNICODE)
    cur = re.sub(r"_+", "_", cur)
    return cur.strip("_")


def make_id(*parts: str) -> str:
    if _graphify_make_id is not None:
        return _graphify_make_id(*parts)
    return normalize_id("_".join(p.strip("_.") for p in parts if p))


def file_stem(relative: Path) -> str:
    """The ID prefix for a file: every path segment, extension dropped."""
    return relative.with_suffix("").as_posix()


RE_CLASS_NAME = re.compile(r"^\s*class_name\s+([A-Za-z_]\w*)")
RE_EXTENDS = re.compile(r"^\s*extends\s+([A-Za-z_]\w*)")
RE_FUNC = re.compile(r"^(\s*)(?:static\s+)?func\s+([A-Za-z_]\w*)\s*\(")
RE_SIGNAL = re.compile(r"^\s*signal\s+([A-Za-z_]\w*)")
RE_CONST = re.compile(r"^(\s*)const\s+([A-Za-z_]\w*)")
RE_VAR = re.compile(r"^(\s*)(?:@export[^\s]*\s+)?(?:static\s+)?var\s+([A-Za-z_]\w*)")
RE_LOAD = re.compile(r"(?:preload|load)\s*\(\s*\"([^\"]+)\"")
RE_BARE_CALL = re.compile(r"(?<![\w.])([A-Za-z_]\w*)\s*\(")
RE_TYPED_CALL = re.compile(r"\b([A-Z]\w*)\.([A-Za-z_]\w*)\s*\(")
# `var _lift: Elevator` and `var _lift := Elevator.new()`. Calling a method on
# a typed variable is how this codebase mostly talks across files, so without
# these the graph has the classes but almost none of the traffic between them.
RE_TYPED_VAR = re.compile(r"^\s*(?:@export[^\s]*\s+)?(?:static\s+)?var\s+([A-Za-z_]\w*)\s*:\s*([A-Z]\w*)")
RE_NEW_VAR = re.compile(r"^\s*(?:static\s+)?var\s+([A-Za-z_]\w*)\s*:=\s*([A-Z]\w*)\.new\s*\(")
RE_INSTANCE_CALL = re.compile(r"(?<![\w.])([a-z_]\w*)\.([A-Za-z_]\w*)\s*\(")
RE_TYPE_USE = re.compile(r"(?<![\w.])([A-Z]\w*)\b")


def strip_comment(line: str) -> str:
    """Drop a trailing `#` comment, ignoring one inside a string literal."""
    out, quote = [], ""
    for i, ch in enumerate(line):
        if quote:
            out.append(ch)
            if ch == quote and line[i - 1 : i] != "\\":
                quote = ""
            continue
        if ch in "\"'":
            quote = ch
            out.append(ch)
            continue
        if ch == "#":
            break
        out.append(ch)
    return "".join(out)


class Script:
    """One .gd file: what it declares and what it reaches for."""

    def __init__(self, path: Path, root: Path) -> None:
        self.path = path
        self.relative = path.relative_to(root)
        # Relative, posix, like graphify's own AST pass, so the merged graph
        # does not carry two spellings of the same file.
        self.source_file = self.relative.as_posix()
        self.stem = file_stem(self.relative)
        self.node_id = make_id(self.stem)
        self.class_name: str | None = None
        self.extends: str | None = None
        self.extends_line = 0
        # name -> (kind, line)
        self.symbols: dict[str, tuple[str, int]] = {}
        # (enclosing func or None, name, line) for every call site
        self.calls: list[tuple[str | None, str, int]] = []
        self.typed_calls: list[tuple[str | None, str, str, int]] = []
        # variable name -> the class_name it was declared as
        self.var_types: dict[str, str] = {}
        self.instance_calls: list[tuple[str | None, str, str, int]] = []
        self.type_uses: set[tuple[str, int]] = set()
        self.loads: list[tuple[str, int]] = []
        self._read()

    def _read(self) -> None:
        text = self.path.read_text(encoding="utf-8", errors="replace")
        current: str | None = None
        func_indent = 0
        for number, raw in enumerate(text.splitlines(), start=1):
            line = strip_comment(raw)
            if not line.strip():
                continue
            indent = len(line) - len(line.lstrip())

            match = RE_FUNC.match(line)
            if match:
                current, func_indent = match.group(2), len(match.group(1))
                self.symbols.setdefault(current, ("function", number))
                continue
            # Dedenting back to declaration level ends the function body.
            if current is not None and indent <= func_indent:
                current = None

            if match := RE_CLASS_NAME.match(line):
                self.class_name = match.group(1)
                self.symbols.setdefault(match.group(1), ("class", number))
                continue
            if match := RE_EXTENDS.match(line):
                self.extends, self.extends_line = match.group(1), number
                continue
            if match := RE_SIGNAL.match(line):
                self.symbols.setdefault(match.group(1), ("signal", number))
                continue
            # Only declarations at the top level are members; the rest are locals.
            if (match := RE_CONST.match(line)) and len(match.group(1)) == 0:
                self.symbols.setdefault(match.group(2), ("constant", number))
            elif (match := RE_VAR.match(line)) and len(match.group(1)) == 0:
                self.symbols.setdefault(match.group(2), ("property", number))

            if match := (RE_TYPED_VAR.match(line) or RE_NEW_VAR.match(line)):
                self.var_types.setdefault(match.group(1), match.group(2))
            for found in RE_INSTANCE_CALL.finditer(line):
                self.instance_calls.append((current, found.group(1), found.group(2), number))
            for found in RE_LOAD.finditer(line):
                self.loads.append((found.group(1), number))
            for found in RE_TYPED_CALL.finditer(line):
                self.typed_calls.append((current, found.group(1), found.group(2), number))
            for found in RE_BARE_CALL.finditer(line):
                self.calls.append((current, found.group(1), number))
            for found in RE_TYPE_USE.finditer(line):
                self.type_uses.add((found.group(1), number))


def extract(root: Path) -> dict:
    scripts = [
        Script(p, root)
        for p in sorted(root.rglob("*.gd"))
        if ".godot" not in p.parts and "bin" not in p.parts
    ]
    by_class = {s.class_name: s for s in scripts if s.class_name}
    by_path = {s.relative.as_posix(): s for s in scripts}

    nodes: list[dict] = []
    edges: list[dict] = []
    seen: set[str] = set()

    def node(nid: str, label: str, source: str, line: int | None) -> None:
        if nid in seen:
            return
        seen.add(nid)
        nodes.append({
            "id": nid, "label": label, "file_type": "code", "source_file": source,
            "source_location": f"L{line}" if line else None, "_origin": "ast",
        })

    def edge(src: str, dst: str, relation: str, source: str, line: int | None) -> None:
        if src == dst or src not in seen or dst not in seen:
            return
        edges.append({
            "source": src, "target": dst, "relation": relation,
            "confidence": "EXTRACTED", "confidence_score": 1.0,
            "source_file": source, "source_location": f"L{line}" if line else None,
            "weight": 1.0, "_origin": "ast",
        })

    # Every declaration first, so an edge written below always has both ends.
    for s in scripts:
        node(s.node_id, s.path.name, s.source_file, None)
        for name, (_kind, line) in s.symbols.items():
            node(make_id(s.stem, name), name, s.source_file, line)

    for s in scripts:
        for name, (_kind, line) in s.symbols.items():
            edge(s.node_id, make_id(s.stem, name), "contains", s.source_file, line)

        # `extends Foo` where Foo is another script in this project.
        if s.extends and s.extends in by_class:
            edge(s.node_id, by_class[s.extends].node_id, "imports", s.source_file, s.extends_line)

        for target, line in s.loads:
            relative = target.removeprefix("res://")
            if other := by_path.get(relative):
                edge(s.node_id, other.node_id, "imports", s.source_file, line)

        for caller, name, line in s.calls:
            if name not in s.symbols or s.symbols[name][0] != "function":
                continue  # Not ours: a builtin, a method on some other object.
            src = make_id(s.stem, caller) if caller else s.node_id
            edge(src, make_id(s.stem, name), "calls", s.source_file, line)

        for caller, type_name, method, line in s.typed_calls:
            other = by_class.get(type_name)
            if other is None or method not in other.symbols:
                continue
            src = make_id(s.stem, caller) if caller else s.node_id
            edge(src, make_id(other.stem, method), "calls", s.source_file, line)

        # A method called on a variable whose declared type we know.
        for caller, variable, method, line in s.instance_calls:
            other = by_class.get(s.var_types.get(variable, ""))
            if other is None or method not in other.symbols:
                continue
            src = make_id(s.stem, caller) if caller else s.node_id
            edge(src, make_id(other.stem, method), "calls", s.source_file, line)

        for type_name, line in s.type_uses:
            other = by_class.get(type_name)
            if other is None or other is s:
                continue
            edge(s.node_id, make_id(other.stem, type_name), "references", s.source_file, line)

    return {"nodes": nodes, "edges": edges, "input_tokens": 0, "output_tokens": 0}


SAMPLE = '''\
class_name Widget
extends Panel

signal poked(how: int)
const LIMIT := 4
var count := 0

var style: UiStyle

func _ready() -> void:
    # A call in a comment() must not count, nor "a string()".
    _poke(LIMIT)
    UiStyle.flat(Color.RED)
    style.flat(Color.RED)

func _poke(times: int) -> void:
    var local := 1
    poked.emit(times + local)
'''

HELPER = '''\
class_name UiStyle
extends RefCounted

static func flat(colour: Color) -> StandardMaterial3D:
    return null
'''


def self_check() -> int:
    """Parse two scripts covering every rule and assert what comes out."""
    import tempfile

    failures = 0

    def check(ok: bool, what: str) -> None:
        nonlocal failures
        if not ok:
            failures += 1
            print("FAILED:", what)

    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        (root / "widget.gd").write_text(SAMPLE, encoding="utf-8")
        (root / "ui.gd").write_text(HELPER, encoding="utf-8")
        result = extract(root)
        ids = {n["id"] for n in result["nodes"]}
        relations = {(e["source"], e["target"], e["relation"]) for e in result["edges"]}

        check("widget" in ids, "the file itself is a node")
        check("widget_widget" in ids, "class_name becomes a node")
        check("widget_ready" in ids and "widget_poke" in ids, "both functions are nodes")
        check("widget_poked" in ids, "a signal is a node")
        check("widget_limit" in ids, "a top-level const is a node")
        check("widget_count" in ids, "a top-level var is a node")
        check("widget_local" not in ids, "a local var inside a function is not")

        check(("widget", "widget_ready", "contains") in relations, "the file contains its functions")
        check(("widget_ready", "widget_poke", "calls") in relations, "a bare call to our own function")
        check(("widget_ready", "ui_flat", "calls") in relations, "a call through a known class_name")
        check("widget_style" in ids, "a typed variable is itself a node")
        check(s_has_var := ("widget", "widget_style", "contains") in relations, "and the file contains it")
        check(("widget", "ui_uistyle", "references") in relations, "using a class_name is a reference")

        commented = [e for e in result["edges"] if e["target"] == "widget_comment"]
        check(not commented, "a call written inside a comment is not an edge")

        # Line numbers are what makes a node clickable, and what marks the AST tier.
        ready = next(n for n in result["nodes"] if n["id"] == "widget_ready")
        check(ready["source_location"] == "L10", "a function carries its line, got %s" % ready["source_location"])
        check(all(n.get("_origin") == "ast" for n in result["nodes"]), "every node is stamped as AST")

    print("self-check: %d failed" % failures)
    return 1 if failures else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", nargs="?", help="folder to scan for .gd files")
    parser.add_argument("--out", help="write the graphify extraction JSON here")
    parser.add_argument("--self-check", action="store_true")
    args = parser.parse_args()

    if args.self_check:
        return self_check()
    if not args.root:
        parser.error("give a root, or --self-check")

    root = Path(args.root).resolve()
    result = extract(root)
    text = json.dumps(result, indent=2, ensure_ascii=False)
    if args.out:
        Path(args.out).write_text(text, encoding="utf-8")
    print(
        "GDScript: %d nodes, %d edges from %d files"
        % (len(result["nodes"]), len(result["edges"]), len(list(root.rglob("*.gd"))))
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
