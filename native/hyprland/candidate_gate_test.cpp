#include "candidate_gate.hpp"
#include <cassert>
#include <cstdio>
#include <cstring>
int main() {
    using namespace sledge::hyprland;
    char path[]="/hdd/sledge-candidate-gate-XXXXXX";
    const int fd=mkstemp(path);assert(fd>=0);
    assert(write(fd,"abc",3)==3);close(fd);
    assert(startupDigest(path)=="ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    bool rejected=false;
    try { (void)startupDigest(path,true); } catch (const std::runtime_error&) { rejected=true; }
    assert(rejected); // user-owned/writable scope cannot qualify
    assert(!sha256Pin("unknown") && sha256Pin(startupDigest(path)));
    assert(unlink(path)==0);
    std::puts("PASS startup SHA256 known vector and untrusted scope rejection");
}
