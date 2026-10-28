"""Tests for tomlpatch: run `python3 -m unittest` in this directory."""

import tomllib
import unittest

from tomlpatch import Doc, format_value

SAMPLE = """\
# pzwm — personal config
[general]
autostart = [
    "awww-daemon",   # wallpaper
    "hypridle",
]
scale = 1.5                  # DPI

[[output]]
name = "eDP-1"               # laptop
scale = 1.5

[[output]]
name = "HDMI-A-1"
position = "1707,0"

[binds]
"Super+Return" = "spawn ghostty +new-window"
"Alt+Shift+0" = "layout 0"   # us
"""


class TomlPatchTest(unittest.TestCase):
    def test_untouched_document_round_trips(self):
        self.assertEqual(Doc(SAMPLE).text(), SAMPLE)

    def test_set_existing_key_keeps_comment(self):
        doc = Doc(SAMPLE)
        doc.set("general", "scale", 1.25)
        self.assertIn("scale = 1.25                  # DPI\n", doc.text())
        self.assertEqual(tomllib.loads(doc.text())["general"]["scale"], 1.25)

    def test_set_new_key_lands_in_its_section(self):
        doc = Doc(SAMPLE)
        doc.set("general", "cursor_size", 28)
        data = tomllib.loads(doc.text())
        self.assertEqual(data["general"]["cursor_size"], 28)
        self.assertNotIn("cursor_size", data["binds"])

    def test_set_creates_missing_section(self):
        doc = Doc(SAMPLE)
        doc.set("canvas", "zoom_step", 1.2)
        self.assertEqual(tomllib.loads(doc.text())["canvas"]["zoom_step"], 1.2)

    def test_multiline_array_is_replaced_whole(self):
        doc = Doc(SAMPLE)
        doc.set("general", "autostart", ["waybar"])
        data = tomllib.loads(doc.text())
        self.assertEqual(data["general"]["autostart"], ["waybar"])
        self.assertEqual(data["general"]["scale"], 1.5)

    def test_array_blocks_are_addressed_by_index(self):
        doc = Doc(SAMPLE)
        doc.set("output", "mirror", "eDP-1", index=1)
        outputs = tomllib.loads(doc.text())["output"]
        self.assertEqual(outputs[1]["mirror"], "eDP-1")
        self.assertNotIn("mirror", outputs[0])

    def test_remove_and_add_array_blocks(self):
        doc = Doc(SAMPLE)
        doc.remove_array_block("output", 0)
        doc.add_array_block("output", {"name": "DP-1", "scale": 1.0})
        names = [o["name"] for o in tomllib.loads(doc.text())["output"]]
        self.assertEqual(names, ["HDMI-A-1", "DP-1"])

    def test_quoted_keys_rename_and_delete(self):
        doc = Doc(SAMPLE)
        doc.rename("binds", "Alt+Shift+0", "Alt+Shift+1", "layout 1")
        doc.delete("binds", "Super+Return")
        binds = tomllib.loads(doc.text())["binds"]
        self.assertEqual(binds, {"Alt+Shift+1": "layout 1"})
        self.assertIn("# us", doc.text())

    def test_strings_with_hash_and_quotes_survive(self):
        doc = Doc(SAMPLE)
        cmd = 'grim -g "$(slurp)" - | wl-copy # not a comment'
        doc.set("binds", "Super+Print", cmd)
        self.assertEqual(tomllib.loads(doc.text())["binds"]["Super+Print"], cmd)

    def test_removing_a_block_keeps_comments_of_the_next_section(self):
        text = '[[output]]\nname = "a"\n\n# Keybindings\n[binds]\n"Super+Q" = "close"\n'
        doc = Doc(text)
        doc.remove_array_block("output", 0)
        self.assertEqual(doc.text(), '# Keybindings\n[binds]\n"Super+Q" = "close"\n')

    def test_literal_and_escaped_keys_are_found(self):
        doc = Doc("[binds]\n'Super+Q' = 'close'\n\"\\U0001F600\" = \"none\"\n")
        doc.set("binds", "Super+Q", "fit")
        doc.delete("binds", "\U0001F600")
        self.assertEqual(tomllib.loads(doc.text())["binds"], {"Super+Q": "fit"})

    def test_missing_array_block_is_created_as_array(self):
        doc = Doc(SAMPLE)
        doc.set("output", "name", "DP-2", index=2)
        names = [o["name"] for o in tomllib.loads(doc.text())["output"]]
        self.assertEqual(names, ["eDP-1", "HDMI-A-1", "DP-2"])

    def test_format_value(self):
        self.assertEqual(format_value(True), "true")
        self.assertEqual(format_value(2.0), "2.0")
        self.assertEqual(format_value([]), "[]")


if __name__ == "__main__":
    unittest.main()
