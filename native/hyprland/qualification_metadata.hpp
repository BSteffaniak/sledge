#pragma once
#include <string_view>
namespace sledge::hyprland {
// Packaging supplies this SHA-256 of its canonical source/toolchain/build
// contract. It is deliberately not a circular digest of the plugin itself.
#ifdef SLEDGE_STOCK_BUILD_ID
inline constexpr std::string_view buildId=SLEDGE_STOCK_BUILD_ID;
#else
inline constexpr std::string_view buildId="";
#endif
inline constexpr bool validBuildId() noexcept {
    if (buildId.size()!=64) return false;
    for (const auto c : buildId) if (!((c>='0' && c<='9') || (c>='a' && c<='f'))) return false;
    return true;
}
inline constexpr unsigned registrationVersion=1, wireVersion=1;
}
