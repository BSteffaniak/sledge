#pragma once
#include <array>
#include <cstdint>
#include <xkbcommon/xkbcommon.h>

namespace sledge::hyprland {
// Synthetic-only state; never update physical or consumer keyboard ownership.
inline xkb_mod_mask_t replacementModifierMask(xkb_state* state,uint8_t requested,uint32_t group) noexcept {
    constexpr std::array<uint32_t,8> sides{42,54,29,97,56,100,125,126};
    // update_mask resets masks, NOT update_key's held-key reference counts.
    // Release the previous synthetic chord before reapplying the next one.
    for (const auto key : sides) xkb_state_update_key(state,key+8,XKB_KEY_UP);
    xkb_state_update_mask(state,0,0,0,0,0,group);
    for (unsigned i=0;i<sides.size();++i)
        xkb_state_update_key(state,sides[i]+8,(requested&(1U<<i)) ? XKB_KEY_DOWN : XKB_KEY_UP);
    return xkb_state_serialize_mods(state,XKB_STATE_MODS_DEPRESSED);
}
} // namespace sledge::hyprland
