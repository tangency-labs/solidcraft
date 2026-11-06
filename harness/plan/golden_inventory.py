"""Read committed golden inventory, never runtime passes or historical difftest reports."""

import collections
import json
from pathlib import Path


def read_json(text, location):
    """Reject malformed inventory before a publisher replaces its previous output."""
    try:
        return json.loads(text)
    except json.JSONDecodeError as exc:
        raise ValueError(f"{location}: invalid JSON") from exc


def load_inventory(root):
    """Return per-unit stored-record metadata and declared known-failure entries.

    No kernel runs here. Counts describe committed expectations, not correctness,
    current CI, or whether a declared known-failing case still exists at runtime.
    """
    directory = Path(root) / "crates/goldens/expected"
    if not directory.exists():
        return {}
    known_path = directory / "known_failing.json"
    known = read_json(known_path.read_text(), known_path) if known_path.exists() else {}
    if not isinstance(known, dict):
        raise ValueError(f"{known_path}: expected a unit-to-case mapping")
    for uid, cases in known.items():
        if not isinstance(cases, dict) or any(not isinstance(reason, str) for reason in cases.values()):
            raise ValueError(f"{known_path}: {uid}: expected case-to-reason strings")
    inventory = {}
    for path in sorted(directory.glob("*.jsonl")):
        uid = path.stem
        info = {"records": 0, "errors": 0, "deferrals": 0, "rejections": 0,
                "deferred_by_unit": collections.Counter(), "known_failures": known.get(uid, {}), "path": path}
        with path.open() as stream:
            header = read_json(next(stream, ""), f"{path}:1")
            if (not isinstance(header, dict) or header.get("unit") != uid
                    or type(header.get("format")) is not int or header["format"] != 1):
                raise ValueError(f"{path}: wrong unit or unsupported golden format")
            previous = None
            for number, line in enumerate(stream, 2):
                location = f"{path}:{number}"
                record = read_json(line, location)
                name = record.get("case") if isinstance(record, dict) else None
                if not isinstance(name, str) or (previous is not None and name <= previous):
                    raise ValueError(f"{location}: case names must be sorted and unique strings")
                previous = name
                if name in info["known_failures"]:
                    raise ValueError(f"{location}: known-failing case also has a golden record")
                for field, counter in (("error", "errors"), ("rejected", "rejections")):
                    value = record.get(field)
                    if value is not None:
                        if not isinstance(value, str):
                            raise ValueError(f"{location}: {field} must be a string or null")
                        info[counter] += 1
                deferred = record.get("deferred")
                if deferred is not None:
                    if not isinstance(deferred, dict) or any(
                            not isinstance(deferred.get(field), str) for field in ("unit", "symbol")):
                        raise ValueError(f"{location}: invalid deferred metadata")
                    info["deferrals"] += 1
                    info["deferred_by_unit"][deferred["unit"]] += 1
                info["records"] += 1
        inventory[uid] = info
    if set(known) - set(inventory):
        raise ValueError(f"{known_path}: known-failure unit has no golden file")
    return inventory
