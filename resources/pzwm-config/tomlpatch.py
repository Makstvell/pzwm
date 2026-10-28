"""Line-based edits of a TOML file that keep comments, order and spacing.

Only what pzwm's config needs: plain `[table]` sections, `[[output]]`
array-of-tables blocks, bare or quoted keys, and values that are strings,
numbers, booleans or arrays of those (arrays may span lines).
"""

import json
import re
import tomllib
from dataclasses import dataclass

_HEADER = re.compile(r"^\s*(\[\[?)\s*([A-Za-z0-9_.-]+)\s*\]\]?\s*(#.*)?$")
_KEY = re.compile(r'^(\s*)("(?:[^"\\]|\\.)*"|\'[^\']*\'|[A-Za-z0-9_-]+)(\s*=\s*)')
_BARE_KEY = re.compile(r"^[A-Za-z0-9_-]+$")


@dataclass
class Section:
    name: str
    array: bool
    header: int  # line index of the header, -1 for the root table
    end: int  # first line after the section


@dataclass
class Entry:
    key: str
    start: int
    end: int  # last line of the value (inclusive)
    value_end_col: int  # column just after the value on line `end`


def format_value(value) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, int):
        return str(value)
    if isinstance(value, float):
        text = repr(value)
        return text if any(c in text for c in ".einf") else text + ".0"
    if isinstance(value, str):
        return json.dumps(value, ensure_ascii=False)
    if isinstance(value, (list, tuple)):
        if not value:
            return "[]"
        items = "".join(f"    {format_value(v)},\n" for v in value)
        return "[\n" + items + "]"
    raise TypeError(f"cannot write {type(value).__name__} to TOML")


def format_key(key: str) -> str:
    return key if _BARE_KEY.match(key) else json.dumps(key, ensure_ascii=False)


def _scan_value(lines, line, col):
    """Returns (line, col) just after the value that starts at line/col."""
    depth = 0
    quote = None
    while line < len(lines):
        text = lines[line]
        while col < len(text):
            c = text[col]
            if quote:
                if c == "\\" and quote == '"':
                    col += 2
                    continue
                if c == quote:
                    quote = None
            elif c in "\"'":
                quote = c
            elif c in "[{":
                depth += 1
            elif c in "]}":
                depth -= 1
                if depth == 0:
                    return line, col + 1
            elif c == "#" or c == "\n":
                if depth == 0:
                    return line, _rstrip_col(text, col)
                break  # comment inside a multi-line array: next line
            col += 1
        if depth == 0 and quote is None:
            return line, _rstrip_col(text, len(text))
        line += 1
        col = 0
    return len(lines) - 1, len(lines[-1]) if lines else 0


def _decode_key(raw: str) -> str:
    """The key a bare, "basic" or 'literal' TOML key stands for."""
    if raw[0] not in "\"'":
        return raw
    try:
        return next(iter(tomllib.loads(f"{raw} = 0")))
    except tomllib.TOMLDecodeError:
        return raw.strip("\"'")


def _rstrip_col(text, col):
    while col > 0 and text[col - 1] in " \t":
        col -= 1
    return col


class Doc:
    def __init__(self, text: str):
        if text and not text.endswith("\n"):
            text += "\n"
        self.lines = text.splitlines(keepends=True)

    def text(self) -> str:
        return "".join(self.lines)

    # ---- structure ---------------------------------------------------------

    def sections(self) -> list[Section]:
        found = []
        current = Section("", False, -1, 0)
        i = 0
        while i < len(self.lines):
            m = _HEADER.match(self.lines[i])
            if m:
                current.end = i
                found.append(current)
                current = Section(m.group(2), m.group(1) == "[[", i, 0)
                i += 1
                continue
            entry = self._entry_at(i)
            i = entry.end + 1 if entry else i + 1
        current.end = len(self.lines)
        found.append(current)
        return found

    def section(self, name: str, index: int = 0) -> Section | None:
        matches = [s for s in self.sections() if s.name == name]
        return matches[index] if index < len(matches) else None

    def count(self, name: str) -> int:
        return sum(1 for s in self.sections() if s.name == name)

    def _entry_at(self, i) -> Entry | None:
        m = _KEY.match(self.lines[i])
        if not m:
            return None
        raw = m.group(2)
        key = _decode_key(raw)
        end, col = _scan_value(self.lines, i, m.end())
        return Entry(key, i, end, col)

    def entries(self, sec: Section) -> list[Entry]:
        found = []
        i = sec.header + 1
        while i < sec.end:
            entry = self._entry_at(i)
            if entry:
                found.append(entry)
                i = entry.end + 1
            else:
                i += 1
        return found

    def find(self, sec: Section, key: str) -> Entry | None:
        return next((e for e in self.entries(sec) if e.key == key), None)

    # ---- edits -------------------------------------------------------------

    def set(self, name: str, key: str, value, index: int = 0):
        """Sets `key` in section `name` (the `index`-th one for arrays)."""
        sec = self.section(name, index)
        if sec is None:
            sec = self._append_section(name, array=name in self._array_names())
        new_value = format_value(value)
        entry = self.find(sec, key)
        if entry:
            prefix = _KEY.match(self.lines[entry.start]).group(0)
            tail = self.lines[entry.end][entry.value_end_col :]
            replaced = (prefix + new_value + tail).splitlines(keepends=True)
            self.lines[entry.start : entry.end + 1] = replaced
            return
        entries = self.entries(sec)
        at = entries[-1].end + 1 if entries else sec.header + 1
        line = f"{format_key(key)} = {new_value}\n"
        self.lines[at:at] = line.splitlines(keepends=True)

    def delete(self, name: str, key: str, index: int = 0):
        sec = self.section(name, index)
        entry = sec and self.find(sec, key)
        if entry:
            del self.lines[entry.start : entry.end + 1]

    def rename(self, name: str, old: str, new: str, value):
        """Replaces key `old` by `new` = `value`, keeping its position."""
        if old != new:
            self.delete(name, new)
        sec = self.section(name)
        entry = sec and self.find(sec, old)
        if not entry:
            self.set(name, new, value)
            return
        indent = _KEY.match(self.lines[entry.start]).group(1)
        tail = self.lines[entry.end][entry.value_end_col :]
        line = f"{indent}{format_key(new)} = {format_value(value)}{tail}"
        self.lines[entry.start : entry.end + 1] = line.splitlines(keepends=True)

    def add_array_block(self, name: str, values: dict):
        self._ensure_blank_line()
        self.lines.append(f"[[{name}]]\n")
        for key, value in values.items():
            self.lines.append(f"{format_key(key)} = {format_value(value)}\n")

    def remove_array_block(self, name: str, index: int):
        sec = self.section(name, index)
        if sec is None:
            return
        entries = self.entries(sec)
        end = entries[-1].end + 1 if entries else sec.header + 1
        # Comments after the last key may describe the next section: keep
        # them. Take only the blank lines that separated the block.
        while end < sec.end and not self.lines[end].strip():
            end += 1
        del self.lines[sec.header : end]

    def _array_names(self) -> set[str]:
        return {s.name for s in self.sections() if s.array}

    def _append_section(self, name: str, array: bool = False) -> Section:
        self._ensure_blank_line()
        self.lines.append(f"[[{name}]]\n" if array else f"[{name}]\n")
        return self.section(name, self.count(name) - 1)

    def _ensure_blank_line(self):
        if self.lines and self.lines[-1].strip():
            self.lines.append("\n")
