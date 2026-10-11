import Gio from "gi://Gio";
import GLib from "gi://GLib";
import { Extension } from "resource:///org/gnome/shell/extensions/extension.js";
import * as Main from "resource:///org/gnome/shell/ui/main.js";
import * as Keyboard from "resource:///org/gnome/shell/ui/status/keyboard.js";

const XML = `<node><interface name="org.sledge.Desktop1">
<method name="Snapshot"><arg direction="out" type="s"/></method>
<method name="Select"><arg direction="in" type="s"/><arg direction="in" type="s"/><arg direction="in" type="s"/><arg direction="out" type="b"/></method>
</interface></node>`;

// GNOME 45-48 ESM ABI. Only fixed source activation, never Eval/arbitrary code.
export default class SledgeDesktop extends Extension {
  enable() {
    this._epoch = GLib.uuid_string_random();
    this._sequence = 0;
    this._manager = Keyboard.getInputSourceManager();
    if (
      !this._manager ||
      !Main.screenShield ||
      !("inputSources" in this._manager) ||
      !("currentSource" in this._manager)
    )
      throw new Error("Unsupported GNOME input-source ABI");
    this._object = Gio.DBusExportedObject.wrapJSObject(XML, this);
    this._object.export(Gio.DBus.session, "/org/sledge/Desktop");
    this._name = Gio.bus_own_name_on_connection(
      Gio.DBus.session,
      "org.sledge.Desktop.Gnome",
      Gio.BusNameOwnerFlags.NONE,
      null,
      null,
    );
  }

  _inventory() {
    return Object.values(this._manager.inputSources).map((source) => ({
      id: JSON.stringify([source.type, source.id]),
      label: source.displayName,
      enabled: true,
    }));
  }

  Snapshot() {
    const sources = this._inventory();
    const source = this._manager.currentSource;
    const window = global.display.focus_window;
    // Meta supplies the class/app_id. No Shell.WindowTracker heuristic or PID/title.
    const identity = window?.get_wm_class();
    const native = window?.get_client_type() === 1; // Meta.WindowClientType.WAYLAND
    const shielded =
      Main.screenShield.active ||
      Main.screenShield.locked ||
      Main.sessionMode.isLocked ||
      Main.overview.visible;
    return JSON.stringify({
      version: 1,
      epoch: this._epoch,
      sequence: ++this._sequence,
      focus:
        !shielded && identity
          ? {
              category: native ? "wayland" : "x11_class",
              value: identity,
            }
          : null,
      lock_hint: shielded ? "locked" : "unknown",
      generation: JSON.stringify(sources),
      sources,
      current: source ? JSON.stringify([source.type, source.id]) : null,
    });
  }

  Select(epoch, generation, id) {
    if (
      epoch !== this._epoch ||
      generation !== JSON.stringify(this._inventory()) ||
      Main.screenShield.active ||
      Main.screenShield.locked ||
      Main.sessionMode.isLocked
    )
      return false;
    const source = Object.values(this._manager.inputSources).find(
      (candidate) => JSON.stringify([candidate.type, candidate.id]) === id,
    );
    if (!source) return false;
    source.activate(true);
    // Client independently reads Snapshot to confirm observed selection.
    return true;
  }

  disable() {
    if (this._name) Gio.bus_unown_name(this._name);
    this._name = 0;
    this._object?.unexport();
    this._object = null;
    this._manager = null;
  }
}
