#pragma once

#include <stdexcept>
#include <string_view>

namespace sledge::hyprland {

// No artifact is qualified. Keep this policy independent of host header globals
// so its fail-closed behavior can be tested without inventing compositor symbols.
inline void requireQualifiedArtifact(std::string_view commit) {
    if (commit.empty() || commit == "unknown")
        throw std::runtime_error("sledge qualification probe: unknown SDK commit; no hooks installed");
    throw std::runtime_error("sledge qualification probe: unqualified artifact; no hooks installed");
}

} // namespace sledge::hyprland
