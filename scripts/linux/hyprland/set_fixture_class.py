"""Set real WM_CLASS on the isolated guest's already-mapped fixture window."""
import ctypes
import os
import sys


class ClassHint(ctypes.Structure):
    _fields_ = [("res_name", ctypes.c_char_p), ("res_class", ctypes.c_char_p)]


def main():
    if os.environ.get("DISPLAY") != ":0":
        raise RuntimeError("guest DISPLAY=:0 required")
    library = os.environ["SLEDGE_FIXTURE_X11_LIBRARY"]
    if not library.startswith("/nix/store/"):
        raise RuntimeError("pinned Xlib required")
    window = int(sys.argv[1], 16)
    if window <= 0:
        raise ValueError("invalid fixture window")
    xlib = ctypes.CDLL(library)
    xlib.XOpenDisplay.argtypes = [ctypes.c_char_p]
    xlib.XOpenDisplay.restype = ctypes.c_void_p
    xlib.XSetClassHint.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.POINTER(ClassHint)]
    xlib.XSetClassHint.restype = ctypes.c_int
    xlib.XSync.argtypes = [ctypes.c_void_p, ctypes.c_int]
    xlib.XCloseDisplay.argtypes = [ctypes.c_void_p]
    display = xlib.XOpenDisplay(b":0")
    if not display:
        raise RuntimeError("guest XWayland display unavailable")
    try:
        hint = ClassHint(b"sledge-xev", b"SledgeFixtureX11")
        if not xlib.XSetClassHint(display, window, ctypes.byref(hint)):
            raise RuntimeError("WM_CLASS write failed")
        xlib.XSync(display, 0)
    finally:
        xlib.XCloseDisplay(display)


if __name__ == "__main__":
    main()
