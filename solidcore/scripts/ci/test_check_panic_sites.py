"""check_panic_sites.py: site keys, proofs, the list ratchet (clippy messages are synthetic)."""
import json
from pathlib import Path, PureWindowsPath
import subprocess
import tempfile
import unittest

import check_panic_sites as c

LIB = "crates/solidcore/src/a.rs"
PREFIXES = ("crates/solidcore/src/",)


def message(lint, text, src, needle, file=LIB, nth=0):
    """A clippy JSON message whose primary span is the nth `needle` in `src`."""
    start = -1
    for _ in range(nth + 1):
        start = src.index(needle, start + 1)
    line = src.count("\n", 0, start) + 1
    col = start - (src.rfind("\n", 0, start) + 1) + 1
    b = len(src[:start].encode())
    span = {
        "file_name": file, "byte_start": b, "byte_end": b + len(needle.encode()),
        "line_start": line, "column_start": col, "is_primary": True, "expansion": None,
    }
    return {"reason": "compiler-message",
            "message": {"code": {"code": lint}, "message": text, "spans": [span]}}


def remove(src, nth=0):
    return message("clippy::disallowed_methods",
                   "use of a disallowed method `alloc::vec::Vec::remove`", src, "remove", nth=nth)


def rows(src, msgs):
    raw = c.raw_sites(msgs, PREFIXES, lambda f: src.encode())
    return c.keyed_sites(raw, lambda f: src)


SRC = """\
impl<T: Copy> Holder<T> for Store<T> where T: Default {
    fn take(&mut self, v: &mut Vec<u8>, i: usize) -> [u8; 2] {
        let s = "fn fake() { v.remove(9) }"; // fn nope() {
        let c = '{';
        v.remove(i);
        v.remove(0);
        [0, 0]
    }
}

fn outer(f: impl Fn(usize) -> usize) {
    fn inner(v: &mut Vec<u8>) {
        v.remove(1);
    }
}
"""


class Keys(unittest.TestCase):
    def test_enclosing_function_paths(self):
        msgs = [remove(SRC, 1), remove(SRC, 2), remove(SRC, 3)]
        got = [(r["fn"], r["n"], r["code"]) for r in rows(SRC, msgs)]
        self.assertEqual(got, [("Store::take", "1", "remove(i)"),
                               ("Store::take", "2", "remove(0)"),
                               ("outer::inner", "1", "remove(1)")])

    def test_key_survives_an_edit_above(self):
        before = rows(SRC, [remove(SRC, 1)])
        edited = "// a new comment\nfn other() {}\n" + SRC
        after = rows(edited, [remove(edited, 1)])
        self.assertEqual([c.key(r) for r in before], [c.key(r) for r in after])

    def test_trait_method_declaration_is_not_a_body(self):
        src = "trait T {\n    fn decl(&self) -> [u8; 2];\n    fn body(v: &mut Vec<u8>) { v.remove(0); }\n}\n"
        self.assertEqual(rows(src, [remove(src)])[0]["fn"], "T::body")

    def test_lifetimes_do_not_swallow_following_code(self):
        # a lifetime has no closing quote: taken for a char literal it would blank the
        # text up to the next quote, the `(` of the signature included
        src = ("fn f<'a>(v: &'a mut Vec<u8>, c: char) -> &'a u8 {\n"
               "    let _q = '{';\n    v.remove(0);\n    &v[0]\n}\n"
               "fn g(v: &mut Vec<u8>) { v.remove(1); }\n")
        got = [(r["fn"], r["code"]) for r in rows(src, [remove(src), remove(src, 1)])]
        self.assertEqual(got, [("f", "remove(0)"), ("g", "remove(1)")])

    def test_other_files_and_lints_are_ignored(self):
        msgs = [message("clippy::disallowed_methods", "use of a disallowed method "
                        "`alloc::vec::Vec::remove`", SRC, "remove", file="crates/x/src/a.rs"),
                message("clippy::needless_return", "unneeded return", SRC, "remove")]
        self.assertEqual(rows(SRC, msgs), [])


class Division(unittest.TestCase):
    SRC = "fn f(a: usize, b: usize, mut v: usize) -> usize {\n    v %= b;\n    (a / b) + a % 7\n}\n"

    def test_operators_and_compound_assignment(self):
        msgs = [
            message("clippy::integer_division_remainder_used",
                    "use of `/` has been disallowed in this context", self.SRC, "a / b"),
            message("clippy::integer_division_remainder_used",
                    "use of `%` has been disallowed in this context", self.SRC, "a % 7"),
            message("clippy::arithmetic_side_effects", "arithmetic operation that can "
                    "potentially result in unexpected side-effects", self.SRC, "v %= b"),
            message("clippy::arithmetic_side_effects", "arithmetic operation that can "
                    "potentially result in unexpected side-effects", self.SRC, "(a / b) + a % 7"),
        ]
        got = sorted((r["pattern"], r["code"]) for r in rows(self.SRC, msgs))
        self.assertEqual(got, [("int-div", "a / b"), ("int-rem", "a % 7"), ("int-rem", "v %= b")])


class Proofs(unittest.TestCase):
    def test_mechanical_rules(self):
        yes = [("int-div", "n / 2"), ("int-rem", "(i + 1) % 3usize"), ("int-div", "x / (4 * 32)"),
               ("int-div", "left / min_words.max(1)"), ("windows", "windows(2)"),
               ("step_by", "step_by(3)"), ("with_capacity", "Vec::with_capacity(16)"),
               ("f64::clamp", "clamp(-1.0, 1.0)"), ("Vec::insert", "insert(0, x)")]
        no = [("int-div", "n / m"), ("int-div", "n / 0"), ("int-div", "n / 2 / m"),
              ("windows", "windows(k)"), ("with_capacity", "Vec::with_capacity(n)"),
              ("f64::clamp", "clamp(1.0, -1.0)"), ("f64::clamp", "clamp(lo, hi)"),
              ("Vec::insert", "insert(i, x)"), ("Vec::remove", "remove(0)"),
              # a compound assignment's divisor is everything after its operator
              ("int-div", "x /= n / 2"), ("int-rem", "x %= y % 3"), ("int-div", "x /= a - b / 2"),
              ("int-div", "x /= y / z.max(1)")]
        yes += [("int-div", "x /= 2"), ("int-rem", "x %= (4 * 32)"), ("int-div", "x /= y.max(1)")]
        for pattern, code in yes:
            self.assertIsNotNone(c.auto_reason(pattern, code), code)
        for pattern, code in no:
            self.assertIsNone(c.auto_reason(pattern, code), code)

    def test_named_constant_divisor(self):
        src = "const BLOCK: usize = 64;\nconst ZERO: usize = 0;\n"
        self.assertIsNotNone(c.auto_reason("int-div", "size / BLOCK", src))
        self.assertIsNone(c.auto_reason("int-div", "size / ZERO", src))
        self.assertIsNone(c.auto_reason("int-div", "size / BLOCK", ""))
        self.assertIsNotNone(c.auto_reason("int-div", "size /= BLOCK", src))
        self.assertIsNone(c.auto_reason("int-div", "size /= n - m / BLOCK", src))

    def test_a_parameter_named_like_the_constant_is_no_proof(self):
        src = "mod a { const BLOCK: usize = 64; }\nfn f(size: usize, BLOCK: usize) {}\n"
        self.assertIsNone(c.auto_reason("int-div", "size / BLOCK", src))
        src = "const BLOCK: usize = 64;\nlet g = |mut BLOCK: usize| 0;\n"
        self.assertIsNone(c.auto_reason("int-div", "size / BLOCK", src))


def row(fn="f", pattern="Vec::remove", n="1", status="open", code="remove(i)", reason="",
        file=LIB):
    return {"file": file, "fn": fn, "pattern": pattern, "n": n, "status": status,
            "code": code, "reason": reason}


class Ratchet(unittest.TestCase):
    def test_matching_list_passes(self):
        self.assertEqual(c.check([row()], [row()]), [])

    def test_new_site_fails(self):
        errors = c.check([row(), row(n="2")], [row()])
        self.assertEqual(len(errors), 1)
        self.assertIn("new panic-prone call", errors[0])

    def test_new_mechanically_proven_site_passes(self):
        for pattern, code in [("windows", "windows(2)"), ("f64::clamp", "clamp(-1.0, 1.0)")]:
            self.assertEqual(c.check([row(pattern=pattern, code=code)], []), [], code)
        self.assertEqual(len(c.check([row(pattern="swap", code="swap(0, 1)")], [])), 1)

    def test_gone_site_is_reported_but_passes(self):
        self.assertEqual(c.check([], [row()]), [])
        self.assertIn("--update", c.stale([], [row()]))
        self.assertIsNone(c.stale([row()], [row()]))

    def test_proven_mark_cannot_slide_onto_other_code(self):
        listed = [row(status="proven", code="remove(0)", reason="guarded")]
        self.assertEqual(c.check([row(code="remove(0)")], listed), [])
        errors = c.check([row(code="remove(j)")], listed)
        self.assertEqual(len(errors), 1)
        self.assertIn("new panic-prone call `remove(j)`", errors[0])

    def test_ratchet_tolerates_a_renumbering_of_open_rows(self):
        # a proven remove(0) added before two open rows shifts both of their n
        base = [row(code="remove(i)"), row(n="2", code="remove(j)")]
        listed = [row(status="proven", code="remove(0)", reason="literal index after a check"),
                  row(n="2", code="remove(i)"), row(n="3", code="remove(j)")]
        self.assertEqual(c.ratchet(listed, base), [])
        # a different call at an existing key is a new open row
        self.assertEqual(len(c.ratchet([row(code="remove(k)")], [row()])), 1)
        # a function renamed keeps its open rows
        self.assertEqual(c.ratchet([row(fn="g")], [row()]), [])

    def test_renumbered_site_is_not_new(self):
        # a provable windows(2) added before a listed open windows(k) shifts its n
        listed = [row(pattern="windows", code="windows(k)")]
        current = [row(pattern="windows", code="windows(2)"),
                   row(pattern="windows", n="2", code="windows(k)")]
        self.assertEqual(c.check(current, listed), [])
        self.assertIsNone(c.stale(current, listed))
        # removing the first of two listed sites renumbers the second: a notice only
        listed = [row(code="remove(i)"), row(n="2", code="remove(j)")]
        self.assertEqual(c.check([row(code="remove(j)")], listed), [])
        self.assertIn("remove", c.stale([row(code="remove(j)")], listed))

    def test_a_second_copy_of_a_listed_call_is_new(self):
        listed = [row(status="proven", code="remove(0)", reason="guarded")]
        current = [row(code="remove(0)"), row(n="2", code="remove(0)")]
        errors = c.check(current, listed)
        self.assertEqual(len(errors), 1)
        self.assertIn("new panic-prone call", errors[0])

    def test_proven_needs_a_reason(self):
        self.assertIn("no reason", c.check([row()], [row(status="proven")])[0])

    def test_new_open_row_against_base_fails(self):
        errors = c.check([row(), row(n="2")], [row(), row(n="2")], base=[row()])
        self.assertEqual(len(errors), 1)
        self.assertIn("gains an open row", errors[0])

    def test_new_proven_row_against_base_passes(self):
        listed = [row(), row(n="2", status="proven", reason="guarded")]
        self.assertEqual(c.check([row(), row(n="2")], listed, base=[row()]), [])

    def test_moved_open_row_matches_base_by_code(self):
        moved = [row(file="crates/solidcore/src/b.rs", fn="g")]
        self.assertEqual(c.check(moved, moved, base=[row()]), [])
        self.assertEqual(len(c.check(moved + [row(file="crates/solidcore/src/b.rs", fn="g",
                                                  n="2")],
                                     moved + [row(file="crates/solidcore/src/b.rs", fn="g",
                                                  n="2")], base=[row()])), 1)


class Update(unittest.TestCase):
    def test_keeps_marks_drops_gone_and_does_not_copy_marks(self):
        listed = [row(n="1", status="proven", code="remove(0)", reason="guarded"),
                  row(fn="g", code="remove(9)")]
        current = [row(n="1", code="remove(0)"), row(fn="h", code="remove(0)"),
                   row(fn="k", pattern="int-div", code="n / 2")]
        out = {(r["fn"], r["pattern"]): (r["status"], r["reason"]) for r in c.update(current, listed)}
        self.assertEqual(out[("f", "Vec::remove")], ("proven", "guarded"))
        self.assertEqual(out[("h", "Vec::remove")], ("open", ""))
        self.assertEqual(out[("k", "int-div")][0], "proven")
        self.assertNotIn(("g", "Vec::remove"), out)

    def test_mark_follows_a_renumbered_site_in_its_function(self):
        listed = [row(n="1", code="remove(i)"),
                  row(n="2", status="proven", code="remove(0)", reason="guarded")]
        current = [row(n="1", code="remove(0)")]  # the first site was fixed
        self.assertEqual(c.update(current, listed)[0]["reason"], "guarded")

    def test_written_list_reads_back(self):
        rs = [row(n="2"), row(status="proven", reason="guarded")]
        self.assertEqual(c.read_list(c.write_list(rs)), sorted(rs, key=lambda r: r["n"]))


class Main(unittest.TestCase):
    def test_clippy_json_run_and_mutation(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "crates/solidcore/src").mkdir(parents=True)
            (root / "plan").mkdir()
            src = "fn f(v: &mut Vec<u8>, i: usize) {\n    v.remove(i);\n}\n"
            (root / LIB).write_text(src)
            js = root / "clippy.json"
            js.write_text(json.dumps(remove(src)) + "\n")
            argv = ["x", tmp, "--clippy-json", str(js)]
            self.assertEqual(c.main(argv + ["--update"]), 0)
            self.assertEqual(c.main(argv), 0)
            src2 = src.replace("v.remove(i);", "v.remove(i);\n    v.remove(0);")
            (root / LIB).write_text(src2)
            js.write_text("\n".join(json.dumps(remove(src2, k)) for k in (0, 1)) + "\n")
            self.assertEqual(c.main(argv), 1)




def commit(repo, files):
    """A git repository at `repo` with one commit holding `files` ({path: text})."""
    for name, text in files.items():
        (repo / name).parent.mkdir(parents=True, exist_ok=True)
        (repo / name).write_text(text)
    git = ["git", "-C", str(repo), "-c", "user.name=t", "-c", "user.email=t@invalid"]
    subprocess.run(git + ["init", "-q"], check=True)
    subprocess.run(git + ["add", "."], check=True)
    subprocess.run(git + ["commit", "-q", "-m", "base"], check=True)


class Paths(unittest.TestCase):
    """#13623: Windows paths, and the list inside a repository it is a folder of."""

    def test_a_windows_span_is_a_library_site(self):
        src = "fn f(v: &mut Vec<u8>, i: usize) {\n    v.remove(i);\n}\n"
        msg = remove(src)
        msg["message"]["spans"][0]["file_name"] = "crates\\solidcore\\src\\a.rs"
        self.assertEqual([r["file"] for r in rows(src, [msg])], [LIB])

    def test_the_base_list_is_read_with_a_windows_path(self):
        with tempfile.TemporaryDirectory() as tmp:
            commit(Path(tmp), {"plan/panic-sites.tsv": c.write_list([row()])})
            list_path = c.LIST
            c.LIST = PureWindowsPath("plan/panic-sites.tsv")
            try:
                self.assertEqual(c.base_list(Path(tmp), "HEAD"), [row()])
            finally:
                c.LIST = list_path

    def test_the_base_list_is_read_in_a_folder_of_the_repository(self):
        with tempfile.TemporaryDirectory() as tmp:
            commit(Path(tmp), {"solidcore/plan/panic-sites.tsv": c.write_list([row()])})
            self.assertEqual(c.base_list(Path(tmp) / "solidcore", "HEAD"), [row()])

    def test_the_base_list_is_read_from_a_commit_of_the_folder_alone(self):
        # CI's base inside SolidCraft: a commit whose tree is `solidcore/` alone.
        with tempfile.TemporaryDirectory() as tmp:
            outer, alone = Path(tmp, "outer"), Path(tmp, "alone")
            alone.mkdir()
            commit(alone, {"plan/panic-sites.tsv": c.write_list([row()])})
            outer.mkdir()
            commit(outer, {"solidcore/plan/panic-sites.tsv": c.write_list([row(n="2")])})
            git = ["git", "-C", str(outer)]
            subprocess.run(git + ["fetch", "-q", str(alone), "HEAD:refs/alone"], check=True)
            self.assertEqual(c.base_list(outer / "solidcore", "refs/alone"), [row()])


class Messages(unittest.TestCase):
    def test_an_unknown_lint_is_an_error(self):
        msg = {"reason": "compiler-message",
               "message": {"message": "unknown lint: `clippy::integer_division_remainder_usd`",
                           "spans": []}}
        with self.assertRaises(SystemExit) as e:
            c.parse_messages(json.dumps(msg))
        self.assertIn("LINTS", str(e.exception))


if __name__ == "__main__":
    unittest.main()
