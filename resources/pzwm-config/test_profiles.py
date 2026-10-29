"""Tests for the Profiles page helpers."""

import json
import unittest

from profiles_page import parse_profiles, profile_summary, valid_name
from pzwm_config import page_from_args


class ValidNameTest(unittest.TestCase):
    def test_accepts_plain_words(self):
        self.assertTrue(valid_name("work-2_b"))

    def test_rejects_spaces_paths_and_empty(self):
        for bad in ("", "my work", "../x", "a/b", "x" * 65):
            self.assertFalse(valid_name(bad), bad)


class SummaryTest(unittest.TestCase):
    def test_lists_windows_apps_and_workspaces(self):
        p = {"windows": 3, "apps": ["org.mozilla.firefox", "foot"], "workspaces": [1, 2], "drawings": 4}
        text = profile_summary(p)
        self.assertIn("3 windows", text)
        self.assertIn("firefox, foot", text)
        self.assertIn("workspaces 1, 2", text)
        self.assertIn("4 drawings", text)

    def test_many_apps_are_shortened(self):
        p = {"windows": 6, "apps": list("abcdef")}
        self.assertIn("a, b, c, d +2", profile_summary(p))


class ParseTest(unittest.TestCase):
    def test_keeps_named_entries_only(self):
        text = json.dumps([{"name": "work"}, {"windows": 1}, "x"])
        self.assertEqual(parse_profiles(text), [{"name": "work"}])

    def test_bad_json_is_an_empty_list(self):
        self.assertEqual(parse_profiles("error: nope"), [])
        self.assertEqual(parse_profiles("{}"), [])


class PageArgTest(unittest.TestCase):
    def test_profiles_page_name_passes_through(self):
        self.assertEqual(page_from_args(["x", "--page", "profiles"]), ("profiles", ["x"]))


if __name__ == "__main__":
    unittest.main()
