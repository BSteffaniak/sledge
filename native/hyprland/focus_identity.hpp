#pragma once
#include "stock_wire.hpp"
#include <string_view>

namespace sledge::hyprland {
// Bounded UTF-8 validation, no truncation/allocation. Unknown is represented by
// kind=0/length=0; malformed or oversized identity must never become an alias.
inline bool validFocusIdentity(std::string_view text) noexcept {
    if (text.empty() || text.size()>256) return false;
    for (std::size_t i=0;i<text.size();) {
        const auto first=static_cast<uint8_t>(text[i++]);
        if (!first) return false;
        if (first<128) continue;
        unsigned remaining=0; uint32_t value=0,minimum=0;
        if (first>=0xC2 && first<=0xDF) { remaining=1;value=first&31;minimum=128; }
        else if (first>=0xE0 && first<=0xEF) { remaining=2;value=first&15;minimum=2048; }
        else if (first>=0xF0 && first<=0xF4) { remaining=3;value=first&7;minimum=65536; }
        else return false;
        if (i+remaining>text.size()) return false;
        while (remaining--) {
            const auto next=static_cast<uint8_t>(text[i++]);
            if ((next&0xC0)!=0x80) return false;
            value=(value<<6)|(next&63);
        }
        if (value<minimum || value>0x10FFFF || (value>=0xD800 && value<=0xDFFF)) return false;
    }
    return true;
}
inline bool encodeFocus(WirePacket& packet,unsigned kind,std::string_view identity) noexcept {
    if ((kind!=1 && kind!=2) || !validFocusIdentity(identity)) return false;
    wire::put(packet,8,4,32+identity.size());wire::put(packet,88,2,kind);wire::put(packet,90,2,identity.size());
    for (unsigned i=0;i<identity.size();++i) packet[96+i]=static_cast<uint8_t>(identity[i]);
    return true;
}
} // namespace sledge::hyprland
