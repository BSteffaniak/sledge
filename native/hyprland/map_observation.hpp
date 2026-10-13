#pragma once
#include <openssl/evp.h>
#include <charconv>
#include <string>
#include <string_view>
namespace sledge::hyprland {
// Query-only stored-map observations. Never called by input callbacks.
inline std::string mapNameJson(std::string_view name) {
    constexpr char hex[]="0123456789abcdef";
    std::string out="\"";
    for (const unsigned char c : name.substr(0,128)) {
        if (c>=32 && c<127 && c!='"' && c!='\\') out+=static_cast<char>(c);
        else { out+="\\u00";out+=hex[c>>4];out+=hex[c&15]; }
    }
    return out+'"';
}
inline bool mapOffset(std::string_view text,unsigned& offset) noexcept {
    constexpr std::string_view command="sledge:maps";
    if (text==command || text.empty()) { offset=0;return true; }
    if (text.starts_with(command)) {
        text.remove_prefix(command.size());
        if (text.empty() || text.front()!=' ') return false;
        text.remove_prefix(1);
    }
    const auto parsed=std::from_chars(text.data(),text.data()+text.size(),offset);
    return parsed.ec==std::errc{} && parsed.ptr==text.data()+text.size() && offset<16;
}
inline std::string storedMapJson(std::string_view bytes,bool present) {
    std::string out="{\"length_bytes\":"+std::to_string(bytes.size())+",\"hash_limit_bytes\":262144,\"status\":\"";
    if (!present || bytes.empty()) return out+"absent\",\"sha256\":null}";
    if (bytes.size()>262144) return out+"oversized\",\"sha256\":null}";
    unsigned char digest[32]{};unsigned length=0;
    if (EVP_Digest(bytes.data(),bytes.size(),digest,&length,EVP_sha256(),nullptr)!=1 || length!=32)
        return out+"digest_failed\",\"sha256\":null}";
    constexpr char hex[]="0123456789abcdef";std::string hash;
    for (const auto c : digest) { hash+=hex[c>>4];hash+=hex[c&15]; }
    return out+"complete\",\"sha256\":\""+hash+"\"}";
}
}
