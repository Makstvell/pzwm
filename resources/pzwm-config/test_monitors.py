"""Tests for the Monitors page helpers and the --page option."""

import unittest

from pages import head_summary, mode_label, mode_text
from pzwm_config import page_from_args

MODE = {"width": 2560, "height": 1440, "refresh": 164.956, "preferred": True}


class ModeTextTest(unittest.TestCase):
    def test_keeps_millihertz_and_drops_trailing_zeros(self):
        self.assertEqual(mode_text(MODE), "2560x1440@164.956")
        self.assertEqual(mode_text({"width": 1920, "height": 1080, "refresh": 60.0}), "1920x1080@60")

    def test_label_marks_the_preferred_mode(self):
        self.assertIn("(preferred)", mode_label(MODE))
        self.assertNotIn("(preferred)", mode_label({**MODE, "preferred": False}))


class HeadSummaryTest(unittest.TestCase):
    def test_enabled_head_shows_mode_scale_and_position(self):
        head = {"model": "DP-1", "enabled": True, "current_mode": 0, "modes": [MODE],
                "scale": 1.25, "x": 1920, "y": 0}
        self.assertEqual(head_summary(head), "DP-1  ·  2560×1440 @ 165 Hz  ·  scale 1.25  ·  at 1920,0")

    def test_disabled_head_says_off(self):
        self.assertEqual(head_summary({"model": "HDMI-A-1", "enabled": False}), "HDMI-A-1  ·  off")


class PageArgTest(unittest.TestCase):
    def test_page_is_taken_out_of_argv(self):
        self.assertEqual(page_from_args(["pzwm-config", "--page", "monitors"]), ("output", ["pzwm-config"]))

    def test_without_page_argv_is_unchanged(self):
        self.assertEqual(page_from_args(["pzwm-config"]), (None, ["pzwm-config"]))
        self.assertEqual(page_from_args(["pzwm-config", "--page"]), (None, ["pzwm-config"]))


if __name__ == "__main__":
    unittest.main()
