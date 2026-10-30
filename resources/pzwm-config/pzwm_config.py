#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Maksym Shymchenko (Makstvell)
"""pzwm config: a settings app for ~/.config/pzwm/config.toml.

Edits keep the file's comments and order (see tomlpatch.py). Saving checks
the new file with `pzwm --check-config` first, keeps a backup next to it
and lets pzwm apply it (pzwm reloads its config when the file changes).
"""

import json
import os
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, Gio, Gtk  # noqa: E402

from pages import BindingsPage, MonitorsPage, section_page  # noqa: E402
from profiles_page import ProfilesPage  # noqa: E402
from schema import PAGES  # noqa: E402
from tomlpatch import Doc  # noqa: E402

APP_ID = "dev.pzwm.Config"
TIMEOUT_S = 5


def config_path() -> Path:
    base = os.environ.get("XDG_CONFIG_HOME") or str(Path.home() / ".config")
    return Path(base) / "pzwm" / "config.toml"


def run_pzwm(*args: str) -> subprocess.CompletedProcess | None:
    """Runs the pzwm binary; None when it is not installed or hangs."""
    if not shutil.which("pzwm"):
        return None
    try:
        return subprocess.run(
            ["pzwm", *args], capture_output=True, text=True, timeout=TIMEOUT_S, check=False
        )
    except (OSError, subprocess.TimeoutExpired):
        return None


class Store:
    """The config being edited: the file as a Doc plus pzwm's defaults."""

    def __init__(self, path: Path):
        # Write through a symlink (dotfiles managed with stow and the like).
        self.path = path.resolve() if path.is_symlink() else path
        self.saved_text = path.read_text() if path.exists() else ""
        self.doc = Doc(self.saved_text)
        printed = run_pzwm("--print-default-config")
        try:
            self.defaults = tomllib.loads(printed.stdout) if printed and printed.returncode == 0 else {}
        except tomllib.TOMLDecodeError:
            self.defaults = {}
        self.on_change = lambda: None

    # ---- reading -----------------------------------------------------------

    def data(self) -> dict:
        return tomllib.loads(self.doc.text())

    def value(self, section: str, key: str):
        mine = self.data().get(section, {})
        if key in mine:
            return mine[key]
        return self.defaults.get(section, {}).get(key)

    def dirty(self) -> bool:
        return self.doc.text() != self.saved_text

    def outputs(self) -> list[dict]:
        return self.data().get("output", [])

    def heads(self) -> list[dict]:
        """Connected monitors with their modes, from the running pzwm."""
        result = run_pzwm("msg", "heads", "--json")
        if not result or result.returncode != 0:
            return []
        try:
            heads = json.loads(result.stdout)
        except json.JSONDecodeError:
            return []
        return heads if isinstance(heads, list) else []

    def connected_outputs(self) -> list[str]:
        return [h.get("name", "") for h in self.heads() if h.get("name")]

    def bindings(self, section: str) -> dict:
        return dict(self.data().get(section, {}))

    def default_bindings(self, section: str) -> dict:
        return dict(self.defaults.get(section, {}))

    # ---- writing -----------------------------------------------------------

    def _changed(self):
        self.on_change()

    def set(self, section: str, key: str, value):
        if self.value(section, key) == value and key not in self.data().get(section, {}):
            return  # same as the default: keep the file short
        self.doc.set(section, key, value)
        self._changed()

    def set_output(self, index: int, key: str, value):
        if value is None:
            self.doc.delete("output", key, index)
        else:
            self.doc.set("output", key, value, index)
        self._changed()

    def add_output(self, name: str):
        self.doc.add_array_block("output", {"name": name})
        self._changed()

    def remove_output(self, index: int):
        self.doc.remove_array_block("output", index)
        self._changed()

    def set_binding(self, section: str, old: str | None, combo: str, action: str):
        if old:
            self.doc.rename(section, old, combo, action)
        else:
            self.doc.set(section, combo, action)
        self._changed()

    def delete_binding(self, section: str, combo: str):
        self.doc.delete(section, combo)
        self._changed()

    def revert(self):
        self.doc = Doc(self.saved_text)
        self._changed()

    def save(self) -> str | None:
        """Writes the file. Returns an error message, or None on success."""
        text = self.doc.text()
        on_disk = self.path.read_text() if self.path.exists() else ""
        if on_disk != self.saved_text:
            return (
                "The config file was changed outside pzwm config. "
                "Use “Reload from disk” in the menu, then make your changes again."
            )
        try:
            tomllib.loads(text)
        except tomllib.TOMLDecodeError as err:
            return f"The file would not be valid TOML: {err}"
        self.path.parent.mkdir(parents=True, exist_ok=True)
        tmp = self.path.with_name(".config.toml.pzwm-config")
        tmp.write_text(text)
        check = run_pzwm("--check-config", str(tmp))
        if check is not None and check.returncode != 0:
            tmp.unlink(missing_ok=True)
            return (check.stdout or check.stderr).strip().replace(str(tmp), str(self.path))
        if self.path.exists():
            shutil.copy2(self.path, self.path.with_name("config.toml.bak"))
        os.replace(tmp, self.path)
        self.saved_text = text
        self._changed()
        return None


class Window(Adw.ApplicationWindow):
    def __init__(self, app: Adw.Application, store: Store):
        super().__init__(application=app, title="pzwm config", default_width=980, default_height=720)
        self.store = store
        self.toasts = Adw.ToastOverlay()
        self.stack = Gtk.Stack(transition_type=Gtk.StackTransitionType.CROSSFADE)

        header = Adw.HeaderBar()
        self.save_button = Gtk.Button(label="Save")
        self.save_button.add_css_class("suggested-action")
        self.save_button.connect("clicked", lambda _b: self.save())
        self.revert_button = Gtk.Button(label="Revert")
        self.revert_button.connect("clicked", lambda _b: self.revert())
        header.pack_end(self.save_button)
        header.pack_end(self.revert_button)
        header.pack_start(self._menu())

        content = Adw.ToolbarView()
        content.add_top_bar(header)
        content.set_content(self.toasts)
        self.toasts.set_child(self.stack)

        self.sidebar = Gtk.ListBox(css_classes=["navigation-sidebar"])
        self.sidebar.connect("row-selected", self._on_row)
        side = Adw.ToolbarView()
        side.add_top_bar(Adw.HeaderBar(title_widget=Adw.WindowTitle(title="pzwm config")))
        side.set_content(Gtk.ScrolledWindow(child=self.sidebar))

        self.split = Adw.NavigationSplitView()
        self.split.set_sidebar(Adw.NavigationPage.new(side, "pzwm config"))
        self.split.set_content(Adw.NavigationPage.new(content, "Settings"))
        self.set_content(self.split)

        narrow = Adw.Breakpoint.new(Adw.BreakpointCondition.parse("max-width: 600sp"))
        narrow.add_setter(self.split, "collapsed", True)
        self.add_breakpoint(narrow)

        self.build_pages()
        store.on_change = self.update_buttons
        self.update_buttons()
        self.connect("close-request", self._on_close)

    def _menu(self) -> Gtk.MenuButton:
        menu = Gio.Menu()
        menu.append("Open config file", "win.open-file")
        menu.append("Reload from disk", "win.reload")
        for name, callback in (("open-file", self._open_file), ("reload", self._reload)):
            action = Gio.SimpleAction.new(name, None)
            action.connect("activate", lambda *_a, cb=callback: cb())
            self.add_action(action)
        return Gtk.MenuButton(icon_name="open-menu-symbolic", menu_model=menu)

    def build_pages(self):
        current = self.stack.get_visible_child_name()
        while (child := self.stack.get_first_child()) is not None:
            self.stack.remove(child)
        while (row := self.sidebar.get_row_at_index(0)) is not None:
            self.sidebar.remove(row)
        pages = [(p.section, section_page(self.store, p)) for p in PAGES]
        pages.insert(3, ("output", MonitorsPage(self.store)))
        pages.append(("binds", BindingsPage(self.store, self, "binds")))
        pages.append(("mouse", BindingsPage(self.store, self, "mouse")))
        pages.append(("profiles", ProfilesPage(self, run_pzwm)))
        for name, page in pages:
            self.stack.add_titled(page, name, page.get_title())
            box = Gtk.Box(spacing=12, margin_top=6, margin_bottom=6, margin_start=6)
            box.append(Gtk.Image.new_from_icon_name(page.get_icon_name()))
            box.append(Gtk.Label(label=page.get_title(), xalign=0))
            row = Gtk.ListBoxRow(child=box)
            row.page_name = name
            self.sidebar.append(row)
        names = [n for n, _ in pages]
        self.sidebar.select_row(self.sidebar.get_row_at_index(names.index(current) if current in names else 0))

    def show_page(self, name: str):
        """Opens a page by section name (`output` for monitors)."""
        index = 0
        while (row := self.sidebar.get_row_at_index(index)) is not None:
            if row.page_name == name:
                self.sidebar.select_row(row)
                return
            index += 1

    def _on_row(self, _list, row):
        if row is not None:
            self.stack.set_visible_child_name(row.page_name)
            self.split.set_show_content(True)

    def update_buttons(self):
        dirty = self.store.dirty()
        self.save_button.set_sensitive(dirty)
        self.revert_button.set_sensitive(dirty)
        self.set_title("pzwm config" + (" •" if dirty else ""))

    def save(self) -> bool:
        error = self.store.save()
        if error:
            dialog = Adw.AlertDialog(heading="Not saved", body=error)
            dialog.add_response("ok", "OK")
            dialog.present(self)
            return False
        auto = self.store.value("general", "auto_reload")
        if auto is False:
            run_pzwm("reload")
        self.toasts.add_toast(Adw.Toast(title="Saved. pzwm applied the changes.", timeout=3))
        return True

    def revert(self):
        self.store.revert()
        self.build_pages()

    def _reload(self):
        self.store = Store(self.store.path)
        self.store.on_change = self.update_buttons
        self.build_pages()
        self.update_buttons()

    def _open_file(self):
        path = self.store.path
        if not path.exists():
            self.save()
        Gio.AppInfo.launch_default_for_uri(path.as_uri(), None)

    def _on_close(self, _window) -> bool:
        if not self.store.dirty():
            return False
        dialog = Adw.AlertDialog(heading="Save changes?", body="Your changes are not saved yet.")
        dialog.add_response("cancel", "Cancel")
        dialog.add_response("discard", "Discard")
        dialog.add_response("save", "Save")
        dialog.set_response_appearance("discard", Adw.ResponseAppearance.DESTRUCTIVE)
        dialog.set_response_appearance("save", Adw.ResponseAppearance.SUGGESTED)

        def on_response(_d, response):
            if response == "discard" or (response == "save" and self.save()):
                self.store.revert()
                self.destroy()

        dialog.connect("response", on_response)
        dialog.present(self)
        return True


# Names `--page` accepts besides the section names.
PAGE_ALIASES = {"monitors": "output", "displays": "output", "shortcuts": "binds"}



def page_from_args(argv: list[str]) -> tuple[str | None, list[str]]:
    """Takes `--page NAME` out of argv (GTK would reject it)."""
    if "--page" not in argv:
        return None, argv
    i = argv.index("--page")
    name = argv[i + 1] if i + 1 < len(argv) else None
    rest = argv[:i] + argv[i + 2 :]
    return (PAGE_ALIASES.get(name, name) if name else None), rest


class App(Adw.Application):
    def __init__(self, page: str | None = None):
        super().__init__(application_id=APP_ID, flags=Gio.ApplicationFlags.HANDLES_COMMAND_LINE)
        self.page = page

    def do_command_line(self, command_line) -> int:
        # A second `pzwm-config --page X` lands here in the running app.
        page, _rest = page_from_args(command_line.get_arguments())
        if page:
            self.page = page
        self.activate()
        return 0

    def do_activate(self):
        window = self.get_active_window()
        if window is None:
            try:
                store = Store(config_path())
                store.data()
            except (OSError, tomllib.TOMLDecodeError) as err:
                self._fatal(f"{config_path()}:\n{err}")
                return
            window = Window(self, store)
        if self.page:
            window.show_page(self.page)
        window.present()

    def _fatal(self, message: str):
        window = Adw.ApplicationWindow(application=self, title="pzwm config")
        window.set_content(
            Adw.StatusPage(
                icon_name="dialog-error-symbolic",
                title="Cannot read the config",
                description=message,
            )
        )
        window.present()


if __name__ == "__main__":
    # The full argv goes to GApplication: with HANDLES_COMMAND_LINE a running
    # instance receives it and opens the asked page (see do_command_line).
    sys.exit(App().run(sys.argv))
