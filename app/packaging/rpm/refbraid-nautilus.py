# Adds "Open in RefBraid" to the right-click menu of folders in Files (Nautilus).
# Works when the nautilus-python package is installed (Nautilus 43 and newer, and older ones).
import subprocess

import gi

try:
    gi.require_version("Nautilus", "4.0")
except ValueError:
    gi.require_version("Nautilus", "3.0")
from gi.repository import GObject, Nautilus


def _open(_item, path):
    subprocess.Popen(["/usr/bin/refbraid", path], start_new_session=True)


def _item(name, path):
    item = Nautilus.MenuItem(name=name, label="Open in RefBraid", icon="refbraid")
    item.connect("activate", _open, path)
    return item


class RefBraidMenu(GObject.GObject, Nautilus.MenuProvider):
    def get_file_items(self, *args):
        files = args[-1]
        if len(files) != 1 or not files[0].is_directory():
            return []
        path = files[0].get_location().get_path()
        return [_item("RefBraid::OpenFolder", path)] if path else []

    def get_background_items(self, *args):
        folder = args[-1]
        path = folder.get_location().get_path()
        return [_item("RefBraid::OpenHere", path)] if path else []
