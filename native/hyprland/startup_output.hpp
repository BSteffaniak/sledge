#pragma once
namespace sledge::hyprland {
// Startup only, after observer hooks and maps exist, before worker admission.
// Ask stock to recompute/replay authority; never infer caps or resource binding.
inline constexpr bool startupReplayAllowed(bool physicalReleased,bool nativeReleased,
    bool exactResources,bool nativeDepressed) noexcept {
    return physicalReleased && nativeReleased && exactResources && !nativeDepressed;
}
template<class Capabilities,class Enter>
void refreshStockOutput(Capabilities capabilities,Enter enter) {
    capabilities();
    enter();
}
}
