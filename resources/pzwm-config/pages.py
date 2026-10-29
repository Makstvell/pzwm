"""Settings pages: one Adw.PreferencesPage per config section."""

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, Gdk, Gtk  # noqa: E402

from schema import KEY_ACTIONS, MOUSE_ACTIONS, Option, Page  # noqa: E402

_MODIFIERS = (
    (Gdk.ModifierType.SUPER_MASK, "Super"),
    (Gdk.ModifierType.CONTROL_MASK, "Ctrl"),
    (Gdk.ModifierType.ALT_MASK, "Alt"),
    (Gdk.ModifierType.SHIFT_MASK, "Shift"),
)
_MODIFIER_KEYS = {
    "Super_L", "Super_R", "Control_L", "Control_R", "Alt_L", "Alt_R",
    "Shift_L", "Shift_R", "Meta_L", "Meta_R", "ISO_Level3_Shift",
}


def _icon_button(icon: str, tooltip: str, on_click) -> Gtk.Button:
    button = Gtk.Button.new_from_icon_name(icon)
    button.set_tooltip_text(tooltip)
    button.set_valign(Gtk.Align.CENTER)
    button.add_css_class("flat")
    button.connect("clicked", lambda _b: on_click())
    return button


def _plain_row(title: str, subtitle: str = "") -> Adw.ActionRow:
    """A row showing user text as is (commands may contain `&` or `<`)."""
    row = Adw.ActionRow()
    row.set_use_markup(False)
    row.set_title(title)
    row.set_subtitle(subtitle)
    return row


def _rgba(text: str) -> Gdk.RGBA:
    color = Gdk.RGBA()
    if not color.parse(text or "#000000"):
        color.parse("#000000")
    return color


def _hex(color: Gdk.RGBA) -> str:
    parts = [color.red, color.green, color.blue]
    if color.alpha < 0.999:
        parts.append(color.alpha)
    return "#" + "".join(f"{round(c * 255):02x}" for c in parts)


def combo_from_event(keyval: int, state: Gdk.ModifierType) -> str | None:
    """`Super+Shift+S` from a key press, or None for a bare modifier."""
    name = Gdk.keyval_name(Gdk.keyval_to_lower(keyval))
    if not name or name in _MODIFIER_KEYS:
        return None
    if len(name) == 1:
        name = name.upper()
    mods = [label for mask, label in _MODIFIERS if state & mask]
    return "+".join([*mods, name])


# ---- option rows -----------------------------------------------------------


def option_row(store, section: str, opt: Option) -> Gtk.Widget:
    value = store.value(section, opt.key)

    def save(new):
        store.set(section, opt.key, new)

    if opt.kind == "bool":
        row = Adw.SwitchRow(title=opt.title, subtitle=opt.subtitle)
        row.set_active(bool(value))
        row.connect("notify::active", lambda r, _p: save(r.get_active()))
        return row
    if opt.kind in ("int", "float"):
        row = Adw.SpinRow.new_with_range(opt.low, opt.high, opt.step)
        row.set_title(opt.title)
        row.set_subtitle(opt.subtitle)
        row.set_digits(0 if opt.kind == "int" else 2)
        row.set_value(float(value if value is not None else opt.low))
        cast = int if opt.kind == "int" else lambda v: round(float(v), 4)
        row.connect("notify::value", lambda r, _p: save(cast(r.get_value())))
        return row
    if opt.kind == "choice":
        row = Adw.ComboRow(title=opt.title, subtitle=opt.subtitle)
        row.set_model(Gtk.StringList.new(list(opt.choices)))
        if value in opt.choices:
            row.set_selected(opt.choices.index(value))
        row.connect("notify::selected", lambda r, _p: save(opt.choices[r.get_selected()]))
        return row
    if opt.kind == "color":
        row = Adw.ActionRow(title=opt.title, subtitle=opt.subtitle)
        button = Gtk.ColorDialogButton.new(Gtk.ColorDialog(with_alpha=True))
        button.set_valign(Gtk.Align.CENTER)
        button.set_rgba(_rgba(value))
        button.connect("notify::rgba", lambda b, _p: save(_hex(b.get_rgba())))
        row.add_suffix(button)
        return row
    row = Adw.EntryRow(title=opt.title)
    row.set_text(str(value or ""))
    if opt.subtitle:
        row.set_tooltip_text(opt.subtitle)
    row.connect("changed", lambda r: save(r.get_text()))
    return row


class ListGroup(Adw.PreferencesGroup):
    """A list of strings (autostart commands), one entry row each."""

    def __init__(self, store, section: str, opt: Option):
        super().__init__(title=opt.title, description=opt.subtitle)
        self.store, self.section, self.key = store, section, opt.key
        self.rows: list[Adw.EntryRow] = []
        self.set_header_suffix(_icon_button("list-add-symbolic", "Add", self.add_item))
        for item in store.value(section, opt.key) or []:
            self._append(item)

    def _append(self, text: str):
        row = Adw.EntryRow(title="Command")
        row.set_text(text)
        row.connect("changed", lambda _r: self._save())
        row.add_suffix(_icon_button("user-trash-symbolic", "Remove", lambda: self._remove(row)))
        self.rows.append(row)
        self.add(row)
        return row

    def add_item(self):
        self._append("").grab_focus()
        self._save()

    def _remove(self, row):
        self.rows.remove(row)
        self.remove(row)
        self._save()

    def _save(self):
        items = [r.get_text().strip() for r in self.rows]
        self.store.set(self.section, self.key, [i for i in items if i])


def section_page(store, page: Page) -> Adw.PreferencesPage:
    prefs = Adw.PreferencesPage(title=page.title, icon_name=page.icon)
    group = Adw.PreferencesGroup()
    prefs.add(group)
    for opt in page.options:
        if opt.kind == "list":
            prefs.add(ListGroup(store, page.section, opt))
            group = Adw.PreferencesGroup()
            prefs.add(group)
        else:
            group.add(option_row(store, page.section, opt))
    return prefs


# ---- monitors --------------------------------------------------------------


def mode_text(mode: dict) -> str:
    """A mode as the config writes it: "2560x1440@165" (refresh rounded to mHz)."""
    refresh = f"{mode.get('refresh', 0):.3f}".rstrip("0").rstrip(".")
    return f"{mode['width']}x{mode['height']}@{refresh}"


def mode_label(mode: dict) -> str:
    label = f"{mode['width']} × {mode['height']}  ·  {mode.get('refresh', 0):.2f} Hz"
    return label + ("  (preferred)" if mode.get("preferred") else "")


def head_summary(head: dict) -> str:
    """One line about a connected monitor: model, current mode, scale."""
    parts = [p for p in (head.get("make", ""), head.get("model", "")) if p]
    modes = head.get("modes", [])
    current = head.get("current_mode")
    if not head.get("enabled"):
        parts.append("off")
    elif isinstance(current, int) and 0 <= current < len(modes):
        m = modes[current]
        parts.append(f"{m['width']}×{m['height']} @ {m.get('refresh', 0):.0f} Hz")
        parts.append(f"scale {head.get('scale', 1):g}")
        parts.append(f"at {head.get('x', 0)},{head.get('y', 0)}")
    return "  ·  ".join(dict.fromkeys(parts))


class MonitorsPage(Adw.PreferencesPage):
    def __init__(self, store):
        super().__init__(title="Monitors", icon_name="video-display-symbolic")
        self.store = store
        self.groups: list[Adw.PreferencesGroup] = []
        self.rebuild()

    def rebuild(self):
        for g in self.groups:
            self.remove(g)
        self.groups = []
        heads = self.store.heads()
        self.heads = {h.get("name", ""): h for h in heads}
        connected = list(self.heads)
        configured = [o.get("name", "") for o in self.store.outputs()]

        intro = Adw.PreferencesGroup(
            title="Connected",
            description="Changes apply as soon as you save. "
            "Monitor tools such as kanshi or wdisplays work too; their changes last until pzwm reloads its config.",
        )
        buttons = Gtk.Box(spacing=6)
        buttons.append(_icon_button("view-refresh-symbolic", "Look for monitors again", self.rebuild))
        buttons.append(_icon_button("list-add-symbolic", "Add a monitor by name", lambda: self._add("")))
        intro.set_header_suffix(buttons)
        for name in connected:
            row = _plain_row(name, head_summary(self.heads[name]))
            if name not in configured:
                row.add_suffix(
                    _icon_button("list-add-symbolic", "Add settings", lambda n=name: self._add(n))
                )
            intro.add(row)
        if not connected:
            intro.add(Adw.ActionRow(title="pzwm is not running", subtitle="No monitor list"))
        self._push(intro)

        # Mirror sources: plugged-in monitors and ones described in the file.
        known = connected + [n for n in configured if n and n not in connected]
        for index, out in enumerate(self.store.outputs()):
            self._push(self._output_group(index, out, known))

    def _push(self, group):
        self.groups.append(group)
        self.add(group)

    def _add(self, name: str):
        self.store.add_output(name)
        self.rebuild()

    def _output_group(self, index: int, out: dict, known: list[str]):
        group = Adw.PreferencesGroup(title=out.get("name") or "New monitor")
        group.set_header_suffix(
            _icon_button("user-trash-symbolic", "Remove these settings", lambda: self._remove(index))
        )

        def text_row(key, title, hint):
            row = Adw.EntryRow(title=title)
            row.set_text(str(out.get(key, "")))
            row.set_tooltip_text(hint)
            row.connect("changed", lambda r: self._set_text(index, key, r))
            group.add(row)

        text_row("name", "Name", "e.g. eDP-1, HDMI-A-1")
        head = self.heads.get(out.get("name", ""))
        if head and head.get("modes"):
            group.add(self._mode_row(index, out, head))
        else:
            text_row("mode", "Mode", "e.g. 2560x1440@165; empty: preferred")
        text_row("scale", "Scale", "e.g. 1.5; empty: [general] scale")
        text_row("position", "Position", "x,y in logical pixels; empty: right of the others")

        others = [n for n in known if n != out.get("name")]
        mirror = out.get("mirror", "")
        if mirror and mirror not in others:
            others.append(mirror)
        choices = ["Off (extend the desktop)", *others]
        row = Adw.ComboRow(title="Mirror", subtitle="Show a copy of another monitor")
        row.set_model(Gtk.StringList.new(choices))
        if mirror in others:
            row.set_selected(others.index(mirror) + 1)
        row.connect("notify::selected", lambda r, _p: self._set_mirror(index, others, r))
        group.add(row)

        off = Adw.SwitchRow(title="Disabled", subtitle="Do not use this monitor")
        off.set_active(bool(out.get("disable", False)))
        off.connect("notify::active", lambda r, _p: self.store.set_output(index, "disable", r.get_active() or None))
        group.add(off)
        return group

    def _mode_row(self, index: int, out: dict, head: dict) -> Adw.ComboRow:
        """Resolution and refresh rate picked from what the monitor offers."""
        modes = head["modes"]
        values = [None, *(mode_text(m) for m in modes)]
        labels = ["Preferred", *(mode_label(m) for m in modes)]
        current = out.get("mode")
        if current and current not in values:
            # Written by hand (e.g. without the refresh): keep it selectable.
            values.append(current)
            labels.append(current)
        row = Adw.ComboRow(title="Mode", subtitle="Resolution and refresh rate")
        row.set_model(Gtk.StringList.new(labels))
        row.set_selected(values.index(current) if current in values else 0)
        row.connect(
            "notify::selected",
            lambda r, _p: self.store.set_output(index, "mode", values[r.get_selected()]),
        )
        return row

    def _set_text(self, index, key, row):
        text = row.get_text().strip()
        row.remove_css_class("error")
        if key == "scale" and text:
            try:
                value = float(text)
            except ValueError:
                row.add_css_class("error")
                return
            self.store.set_output(index, key, value)
            return
        self.store.set_output(index, key, text or None)

    def _set_mirror(self, index, others, row):
        selected = row.get_selected()
        self.store.set_output(index, "mirror", others[selected - 1] if selected > 0 else None)

    def _remove(self, index):
        self.store.remove_output(index)
        self.rebuild()


# ---- bindings --------------------------------------------------------------


class BindingsPage(Adw.PreferencesPage):
    """Keyboard ([binds]) or mouse ([mouse]) bindings."""

    def __init__(self, store, window, section: str):
        mouse = section == "mouse"
        super().__init__(
            title="Mouse" if mouse else "Shortcuts",
            icon_name="input-mouse-symbolic" if mouse else "preferences-desktop-keyboard-shortcuts-symbolic",
        )
        self.store, self.window, self.section, self.mouse = store, window, section, mouse
        self.groups: list[Adw.PreferencesGroup] = []
        self.rebuild()

    def rebuild(self):
        for g in self.groups:
            self.remove(g)
        self.groups = []
        mine = self.store.bindings(self.section)
        builtin = self.store.default_bindings(self.section)

        own = Adw.PreferencesGroup(
            title="Your bindings",
            description="Added to the built-in ones; the same shortcut replaces a built-in one.",
        )
        own.set_header_suffix(_icon_button("list-add-symbolic", "Add", lambda: self._edit(None, "", "")))
        for combo, action in mine.items():
            row = _plain_row(combo, action or "")
            row.add_suffix(_icon_button("document-edit-symbolic", "Edit", lambda c=combo, a=action: self._edit(c, c, a)))
            row.add_suffix(_icon_button("user-trash-symbolic", "Remove", lambda c=combo: self._delete(c)))
            own.add(row)
        self._push(own)

        defaults = Adw.PreferencesGroup(title="Built-in")
        for combo, action in builtin.items():
            if combo in mine:
                continue
            row = _plain_row(combo, action)
            row.add_suffix(_icon_button("document-edit-symbolic", "Change", lambda c=combo, a=action: self._edit(None, c, a)))
            row.add_suffix(_icon_button("action-unavailable-symbolic", "Turn off", lambda c=combo: self._turn_off(c)))
            defaults.add(row)
        self._push(defaults)

    def _push(self, group):
        self.groups.append(group)
        self.add(group)

    def _delete(self, combo):
        self.store.delete_binding(self.section, combo)
        self.rebuild()

    def _turn_off(self, combo):
        self.store.set_binding(self.section, None, combo, "none")
        self.rebuild()

    def _edit(self, old: str | None, combo: str, action: str):
        dialog = Adw.AlertDialog(
            heading="Mouse binding" if self.mouse else "Shortcut",
            body="Mouse: Mods+left, right, middle, back, forward or scroll."
            if self.mouse
            else "Type it, or press Record and then the keys.",
        )
        box = Gtk.ListBox(selection_mode=Gtk.SelectionMode.NONE)
        box.add_css_class("boxed-list")
        combo_row = Adw.EntryRow(title="Shortcut")
        combo_row.set_text(combo)
        box.append(combo_row)
        if not self.mouse:
            record = Gtk.ToggleButton(label="Record", valign=Gtk.Align.CENTER)
            combo_row.add_suffix(record)
            keys = Gtk.EventControllerKey(propagation_phase=Gtk.PropagationPhase.CAPTURE)

            def on_key(_c, keyval, _code, state):
                if not record.get_active():
                    return False
                text = combo_from_event(keyval, state)
                if text:
                    combo_row.set_text(text)
                    record.set_active(False)
                return True

            keys.connect("key-pressed", on_key)
            dialog.add_controller(keys)

        action_row = Adw.EntryRow(title="Action")
        action_row.set_text(action)
        menu = Gtk.MenuButton(icon_name="view-more-symbolic", valign=Gtk.Align.CENTER, tooltip_text="Actions")
        menu.add_css_class("flat")
        menu.set_popover(self._action_popover(action_row))
        action_row.add_suffix(menu)
        box.append(action_row)
        dialog.set_extra_child(box)

        dialog.add_response("cancel", "Cancel")
        dialog.add_response("save", "Save")
        dialog.set_response_appearance("save", Adw.ResponseAppearance.SUGGESTED)
        dialog.set_default_response("save")

        def on_response(_d, response):
            new_combo = combo_row.get_text().strip()
            new_action = action_row.get_text().strip()
            if response == "save" and new_combo and new_action:
                self.store.set_binding(self.section, old, new_combo, new_action)
                self.rebuild()

        dialog.connect("response", on_response)
        dialog.present(self.window)

    def _action_popover(self, target: Adw.EntryRow) -> Gtk.Popover:
        popover = Gtk.Popover()
        scroller = Gtk.ScrolledWindow(propagate_natural_height=True, max_content_height=360)
        listbox = Gtk.ListBox(selection_mode=Gtk.SelectionMode.NONE)
        for name in MOUSE_ACTIONS if self.mouse else KEY_ACTIONS:
            button = Gtk.Button(label=name)
            button.add_css_class("flat")

            def pick(_b, n=name):
                target.set_text(n)
                target.set_position(-1)
                popover.popdown()

            button.connect("clicked", pick)
            listbox.append(button)
        scroller.set_child(listbox)
        popover.set_child(scroller)
        return popover
