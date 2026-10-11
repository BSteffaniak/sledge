#include "identity_policy.hpp"

#include <dlfcn.h>
#include <iostream>
#include <string>

int main(int argc, char** argv) {
    if (argc != 2)
        return 2;
    for (const auto commit : {"", "unknown", "386376400119dd46a767c9f8c8791fd22c7b6e61"}) {
        bool rejected = false;
        try {
            sledge::hyprland::requireQualifiedArtifact(commit);
        } catch (const std::runtime_error& error) {
            const std::string expected = std::string(commit).empty() || std::string(commit) == "unknown"
                ? "sledge qualification probe: unknown SDK commit; no hooks installed"
                : "sledge qualification probe: unqualified artifact; no hooks installed";
            rejected = error.what() == expected;
        }
        if (!rejected)
            return 1;
    }
    std::cout << "PASS: empty, unknown, and known-but-unqualified artifact policy rejects\n";

    // Load failure must not be confused with an executed pluginInit refusal.
    // No host symbol stubs and no RTLD_LAZY hiding of unresolved dependencies.
    void* library = dlopen(argv[1], RTLD_NOW | RTLD_LOCAL);
    if (!library) {
        std::cerr << "BLOCKED: real host required for plugin load: " << dlerror() << '\n';
        return 77;
    }
    if (dlclose(library) != 0)
        return 1;
    std::cerr << "BLOCKED: compositor pluginInit/load qualification not performed\n";
    return 77;
}
