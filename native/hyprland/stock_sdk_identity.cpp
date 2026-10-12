// Standalone exact SDK hash helper, no compositor symbols or hook installation.
// PluginAPI.hpp has global color objects referencing compositor-only constructors;
// avoid linking that header into an executable. This copies ONLY its inline hash
// algorithm and uses the same SDK-generated version macros (0.52.2 lines 322-334).
#include <hyprland/src/version.h>
#include <cstdio>
#include <string>
#include <string_view>
namespace {
std::string stripPatch(const char* version) {
    const std::string_view value=version;
    if (!value.contains('.')) return std::string(value);
    return std::string(value.substr(0,value.find_last_of('.')));
}
}
int main() {
    const auto hash=std::string(GIT_COMMIT_HASH)+"_aq_"+stripPatch(AQUAMARINE_VERSION)+
        "_hu_"+stripPatch(HYPRUTILS_VERSION)+"_hg_"+stripPatch(HYPRGRAPHICS_VERSION)+
        "_hc_"+stripPatch(HYPRCURSOR_VERSION)+"_hlg_"+stripPatch(HYPRLANG_VERSION);
    if (hash.empty() || hash.size()>192) return 1;
    for (const auto value : hash) if (value<32 || value>126) return 1;
    puts(hash.c_str());
}
