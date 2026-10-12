#pragma once
namespace sledge::hyprland {
// Complete preflight must precede this primitive. Failure injection occurs only
// after observable stages; never produce a target up unless down was invoked.
template<class Mods,class Key,class Restore> bool emitReplacement(Mods&& requested,Key&& key,Restore&& restored,unsigned failAfter) {
    requested();
    const bool downInvoked=failAfter!=1;
    if (downInvoked) key(true);
    if (downInvoked) key(false);
    restored();
    return failAfter!=1 && failAfter!=2;
}
} // namespace sledge::hyprland
