#include "map_observation.hpp"
#include <cassert>
int main() {
    using namespace sledge::hyprland;
    assert(mapNameJson("a\"\\\n")=="\"a\\u0022\\u005c\\u000a\"");
    assert(mapNameJson(std::string(1000,'\n')).size()==770);
    unsigned offset=0;
    assert(mapOffset("sledge:maps",offset) && offset==0);
    assert(mapOffset("sledge:maps 15",offset) && offset==15);
    assert(!mapOffset("sledge:maps 16",offset));
    assert(!mapOffset("sledge:maps 18446744073709551616",offset));
    assert(!mapOffset("sledge:maps-extra",offset));
    const auto actual=storedMapJson("abc",true);
    assert(actual.find("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")!=std::string::npos);
    assert(actual.find("\"length_bytes\":3")!=std::string::npos);
    assert(storedMapJson("abc",false).find("absent")!=std::string::npos);
    assert(storedMapJson("",true).find("absent")!=std::string::npos);
    assert(storedMapJson(std::string(262144,'a'),true).find("complete")!=std::string::npos);
    const auto huge=storedMapJson(std::string(262145,'a'),true);
    assert(huge.find("oversized")!=std::string::npos && huge.find("\"sha256\":null")!=std::string::npos);
    assert(actual.size()+mapNameJson(std::string(128,'\n')).size()+1024<8192);
}
