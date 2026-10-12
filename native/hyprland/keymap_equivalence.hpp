#pragma once
#include <xkbcommon/xkbcommon.h>
#include <cstdlib>
#include <cstring>

namespace sledge::hyprland {
// Lifecycle only: serialization allocates. Keymaps created separately per device
// can be equivalent; pointer identity is not a layout/admission qualification.
// Select only the verified active typing consumer, never the first enumerated
// media device. Lifecycle callers provide SDK/libinput qualification facts.
inline xkb_keymap* consumerKeymap(xkb_keymap* active,bool physical,bool enabled,
                                  bool allowed,bool hasA,bool hasShift) noexcept {
    return physical && enabled && allowed && hasA && hasShift ? active : nullptr;
}
inline bool equivalentKeymap(xkb_keymap* first, xkb_keymap* second) {
    if (!first || !second) return false;
    if (first==second) return true;
    auto* left=xkb_keymap_get_as_string(first,XKB_KEYMAP_FORMAT_TEXT_V1);
    auto* right=xkb_keymap_get_as_string(second,XKB_KEYMAP_FORMAT_TEXT_V1);
    const bool equal=left && right && std::strcmp(left,right)==0;
    std::free(left); std::free(right);
    return equal;
}
} // namespace sledge::hyprland
