# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Maksym Shymchenko (Makstvell)

"""The Profiles page: save, load, rename and delete desktop profiles.

Unlike the other pages it does not edit config.toml: every button talks
to the running pzwm right away through `pzwm msg profile …`.
"""

import json
import re
import time

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, Gtk  # noqa: E402

NAME_RE = re.compile(r"^[A-Za-z0-9_-]{1,64}$")
BAD_NAME = "Use letters, digits, - and _ (no spaces)."


def valid_name(name: str) -> bool:
    """Profile names become file names: letters, digits, - and _ only."""
    return bool(NAME_RE.match(name))


def profile_summary(p: dict) -> str:
    """`3 windows · firefox, foot · workspaces 1, 2 · saved 28 Oct 14:05`."""
    parts = [f"{p.get('windows', 0)} windows"]
    apps = p.get("apps") or []
    if apps:
        shown = ", ".join(a.rsplit(".", 1)[-1] for a in apps[:4])
        parts.append(shown + (f" +{len(apps) - 4}" if len(apps) > 4 else ""))
    workspaces = p.get("workspaces") or []
    if workspaces:
        label = "workspaces " if len(workspaces) > 1 else "workspace "
        parts.append(label + ", ".join(map(str, workspaces)))
    if p.get("drawings"):
        parts.append(f"{p['drawings']} drawings")
    if p.get("modified"):
        parts.append("saved " + time.strftime("%d %b %H:%M", time.localtime(p["modified"])))
    return "  ·  ".join(parts)


def parse_profiles(text: str) -> list[dict]:
    try:
        data = json.loads(text)
    except json.JSONDecodeError:
        return []
    if not isinstance(data, list):
        return []
    return [p for p in data if isinstance(p, dict) and p.get("name")]


class ProfilesPage(Adw.PreferencesPage):
    def __init__(self, window, run_pzwm):
        super().__init__(title="Profiles", icon_name="view-grid-symbolic")
        self.window = window
        self.run = run_pzwm
        self.groups: list[Adw.PreferencesGroup] = []
        self.name_entry: Adw.EntryRow | None = None
        self.rebuild()
        # Opened with Super+Shift+P to save: the name can be typed at once.
        self.connect("map", lambda _page: self.name_entry and self.name_entry.grab_focus())

    # ---- talking to pzwm ---------------------------------------------------

    def _msg(self, *args: str) -> tuple[bool, str]:
        result = self.run("msg", "profile", *args)
        if result is None:
            return False, "pzwm is not running"
        text = (result.stdout or result.stderr).strip()
        ok = result.returncode == 0 and not text.startswith("error")
        return ok, text.removeprefix("error: ")

    def _do(self, *args: str):
        ok, text = self._msg(*args)
        self.window.toasts.add_toast(Adw.Toast(title=text or "done", timeout=3))
        if ok:
            self.rebuild()

    # ---- building ----------------------------------------------------------

    def rebuild(self):
        for g in self.groups:
            self.remove(g)
        self.groups = []

        save = Adw.PreferencesGroup(
            title="Save the desktop",
            description="A profile remembers every app with its workspace, place and size, "
            "the view of each workspace and the drawings. Loading it puts open windows back "
            "and starts the apps that are not running.",
        )
        entry = Adw.EntryRow(title="New profile name, e.g. work — press Enter")
        entry.set_show_apply_button(True)
        entry.connect("apply", self._save_new)
        save.add(entry)
        self.name_entry = entry
        self._push(save)

        ok, text = self._msg("list", "--json")
        profiles = parse_profiles(text) if ok else []
        listing = Adw.PreferencesGroup(title="Saved profiles")
        refresh = Gtk.Button.new_from_icon_name("view-refresh-symbolic")
        refresh.add_css_class("flat")
        refresh.set_tooltip_text("Read the list again")
        refresh.connect("clicked", lambda _b: self.rebuild())
        listing.set_header_suffix(refresh)
        if not ok:
            listing.add(Adw.ActionRow(title="pzwm is not running", subtitle=text))
        elif not profiles:
            listing.add(Adw.ActionRow(title="No profiles yet", subtitle="Type a name above and press Enter"))
        for p in profiles:
            listing.add(self._profile_row(p))
        self._push(listing)

    def _push(self, group):
        self.groups.append(group)
        self.add(group)

    def _profile_row(self, p: dict) -> Adw.ActionRow:
        name = p["name"]
        row = Adw.ActionRow()
        row.set_use_markup(False)
        row.set_title(f"{name} (saved automatically)" if p.get("autosave") else name)
        row.set_subtitle(profile_summary(p))
        load = Gtk.Button(label="Load", valign=Gtk.Align.CENTER)
        load.add_css_class("suggested-action")
        load.connect("clicked", lambda _b: self._do("load", name))
        row.add_suffix(load)
        buttons = (
            ("document-save-symbolic", "Save the current desktop into it", lambda: self._confirm_overwrite(name)),
            ("document-edit-symbolic", "Rename", lambda: self._rename(name)),
            ("user-trash-symbolic", "Delete", lambda: self._confirm_delete(name)),
        )
        for icon, tip, callback in buttons:
            button = Gtk.Button.new_from_icon_name(icon)
            button.add_css_class("flat")
            button.set_valign(Gtk.Align.CENTER)
            button.set_tooltip_text(tip)
            button.connect("clicked", lambda _b, cb=callback: cb())
            row.add_suffix(button)
        return row

    # ---- actions -----------------------------------------------------------

    def _save_new(self, entry: Adw.EntryRow):
        name = entry.get_text().strip()
        if not valid_name(name):
            self._alert("Bad name", BAD_NAME)
            return
        self._do("save", name)

    def _confirm_overwrite(self, name: str):
        self._ask(f"Replace “{name}”?", "It will hold the desktop as it is now.", "Replace",
                  lambda: self._do("save", name))

    def _confirm_delete(self, name: str):
        self._ask(f"Delete “{name}”?", "This cannot be undone.", "Delete",
                  lambda: self._do("delete", name), destructive=True)

    def _rename(self, name: str):
        dialog = Adw.AlertDialog(heading=f"Rename “{name}”")
        entry = Gtk.Entry(text=name, activates_default=True)
        dialog.set_extra_child(entry)
        dialog.add_response("cancel", "Cancel")
        dialog.add_response("rename", "Rename")
        dialog.set_default_response("rename")
        dialog.set_response_appearance("rename", Adw.ResponseAppearance.SUGGESTED)

        def on_response(_d, response):
            new = entry.get_text().strip()
            if response != "rename" or new == name:
                return
            if not valid_name(new):
                self._alert("Bad name", BAD_NAME)
                return
            self._do("rename", name, new)

        dialog.connect("response", on_response)
        dialog.present(self.window)

    def _ask(self, heading: str, body: str, verb: str, then, destructive: bool = False):
        dialog = Adw.AlertDialog(heading=heading, body=body)
        dialog.add_response("cancel", "Cancel")
        dialog.add_response("go", verb)
        look = Adw.ResponseAppearance.DESTRUCTIVE if destructive else Adw.ResponseAppearance.SUGGESTED
        dialog.set_response_appearance("go", look)
        dialog.connect("response", lambda _d, r: then() if r == "go" else None)
        dialog.present(self.window)

    def _alert(self, heading: str, body: str):
        dialog = Adw.AlertDialog(heading=heading, body=body)
        dialog.add_response("ok", "OK")
        dialog.present(self.window)
