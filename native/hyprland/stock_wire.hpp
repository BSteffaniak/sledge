#pragma once
#include "decision_mailbox.hpp"
#include <span>
#include <string_view>

namespace sledge::hyprland::wire {
inline uint64_t get(const WirePacket& packet, unsigned offset, unsigned count) noexcept {
    uint64_t value = 0;
    for (unsigned i = 0; i < count; ++i) value |= uint64_t{packet[offset+i]} << (8*i);
    return value;
}
inline void put(WirePacket& packet, unsigned offset, unsigned count, uint64_t value) noexcept {
    for (unsigned i = 0; i < count; ++i) packet[offset+i] = static_cast<uint8_t>(value >> (8*i));
}
inline bool zero(const WirePacket& packet, unsigned begin, unsigned end) noexcept {
    for (unsigned i = begin; i < end; ++i) if (packet[i]) return false;
    return true;
}
inline bool header(const WirePacket& packet, unsigned opcode, unsigned body) noexcept {
    return packet[0]=='S' && packet[1]=='H' && packet[2]=='P' && packet[3]=='1' &&
        get(packet,4,2)==1 && get(packet,6,2)==opcode && get(packet,8,4)==body &&
        zero(packet,12,16) && zero(packet,56,64) && zero(packet,64+body,512);
}
inline bool correlated(const WirePacket& request, const WirePacket& reply) noexcept {
    // Includes device generation, epoch, sequence, deadline and connection nonce.
    for (unsigned i = 16; i < 56; ++i) if (request[i] != reply[i]) return false;
    return true;
}
inline bool decision(const WirePacket& event, const WirePacket& reply) noexcept {
    if (!header(reply,4,16) || !correlated(event,reply) || !zero(reply,76,80)) return false;
    const auto verdict = get(reply,64,4);
    if (verdict > 2) return false;
    if (verdict != 2) return zero(reply,68,76);
    return get(reply,68,4)<768 && get(reply,72,4)<256;
}
enum class Opcode : unsigned { Event=3, Outcome=5, Reset=7, Retire=8, Lock=9 };
struct Scope { uint64_t device,epoch,sequence,deadline,nonce; };
inline void envelope(WirePacket& packet,Opcode opcode,unsigned length,Scope scope) noexcept {
    packet={};packet[0]='S';packet[1]='H';packet[2]='P';packet[3]='1';
    put(packet,4,2,1);put(packet,6,2,static_cast<unsigned>(opcode));put(packet,8,4,length);
    put(packet,16,8,scope.device);put(packet,24,8,scope.epoch);put(packet,32,8,scope.sequence);
    put(packet,40,8,scope.deadline);put(packet,48,8,scope.nonce);
}
inline bool control(WirePacket& packet,Opcode opcode,Scope scope,unsigned value) noexcept {
    if (!scope.nonce || !scope.sequence || !scope.deadline ||
        (opcode!=Opcode::Reset && opcode!=Opcode::Retire && opcode!=Opcode::Lock) ||
        (opcode==Opcode::Lock && (value>2 || scope.device)) ||
        (opcode==Opcode::Reset && (value>8 || (value==8 && scope.device))) ||
        (opcode==Opcode::Retire && (value>7 || !scope.device))) return false;
    envelope(packet,opcode,8,scope);put(packet,64,4,value);return true;
}
inline bool eventPacket(WirePacket& packet,Scope scope,unsigned key,unsigned state,
                        unsigned time,unsigned mods,uint64_t focus) noexcept {
    if (!scope.device || !scope.nonce || !scope.sequence || !scope.deadline || key>=768 || state>2 || mods>255) return false;
    envelope(packet,Opcode::Event,32,scope);
    put(packet,64,4,key);put(packet,68,4,state);put(packet,72,4,time);put(packet,76,4,mods);put(packet,80,8,focus);
    return true;
}
inline bool hello(WirePacket& packet, uint64_t nonce, std::string_view hash, std::string_view artifact) noexcept {
    if (!nonce || hash.empty() || artifact.empty() || hash.size()>192 || artifact.size()>192) return false;
    // Our immutable ASCII identifiers are a strict UTF-8 subset, never daemon
    // strings or credentials. Passive advertisement is always capabilities=0.
    for (auto value : hash) if (value < 32 || value > 126) return false;
    for (auto value : artifact) if (value < 32 || value > 126) return false;
    packet = {};
    packet[0]='S'; packet[1]='H'; packet[2]='P'; packet[3]='1';
    put(packet,4,2,1); put(packet,6,2,1); put(packet,8,4,400); put(packet,48,8,nonce);
    put(packet,72,2,hash.size()); put(packet,74,2,artifact.size());
    for (unsigned i=0; i<hash.size(); ++i) packet[80+i]=static_cast<uint8_t>(hash[i]);
    for (unsigned i=0; i<artifact.size(); ++i) packet[272+i]=static_cast<uint8_t>(artifact[i]);
    return true;
}
inline bool helloAck(const WirePacket& hello, const WirePacket& ack) noexcept {
    // Passive negotiation accepts only the exact identity echo with caps zero.
    if (!header(ack,2,400) || !correlated(hello,ack)) return false;
    for (unsigned i=64; i<464; ++i) if (hello[i]!=ack[i]) return false;
    return true;
}
} // namespace sledge::hyprland::wire
