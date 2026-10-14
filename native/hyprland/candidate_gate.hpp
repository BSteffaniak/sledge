#pragma once
#include <openssl/evp.h>
#include <array>
#include <string>
#include <string_view>
#include <stdexcept>
#include <fcntl.h>
#include <unistd.h>
#include <sys/stat.h>
namespace sledge::hyprland {
inline std::string startupDigest(const char* path,bool immutableScope=false) {
    const int fd=open(path,O_RDONLY|O_CLOEXEC|(immutableScope ? O_NOFOLLOW : 0));
    if (fd<0) throw std::runtime_error("candidate identity file unavailable");
    struct stat metadata{};
    if (fstat(fd,&metadata)!=0 || !S_ISREG(metadata.st_mode) || metadata.st_size<0 ||
        metadata.st_size>(immutableScope ? 4096 : 128*1024*1024) ||
        (immutableScope && (metadata.st_uid!=0 || (metadata.st_mode&0222)))) {
        close(fd);throw std::runtime_error("candidate scope file is not root-owned read-only regular file");
    }
    auto* context=EVP_MD_CTX_new();
    if (!context) { close(fd);throw std::runtime_error("candidate digest allocation failed"); }
    bool good=EVP_DigestInit_ex(context,EVP_sha256(),nullptr)==1;
    std::array<unsigned char,16384> buffer{};ssize_t count=0;std::size_t total=0;
    while (good && (count=read(fd,buffer.data(),buffer.size()))>0) {
        total+=static_cast<std::size_t>(count);
        good=total<=static_cast<std::size_t>(metadata.st_size) &&
            EVP_DigestUpdate(context,buffer.data(),static_cast<std::size_t>(count))==1;
    }
    std::array<unsigned char,32> digest{};unsigned size=0;
    good=good && count==0 && EVP_DigestFinal_ex(context,digest.data(),&size)==1 && size==32;
    struct stat after{};
    good=good && fstat(fd,&after)==0 && after.st_size==metadata.st_size &&
        after.st_mtim.tv_sec==metadata.st_mtim.tv_sec && after.st_mtim.tv_nsec==metadata.st_mtim.tv_nsec &&
        after.st_ctim.tv_sec==metadata.st_ctim.tv_sec && after.st_ctim.tv_nsec==metadata.st_ctim.tv_nsec;
    EVP_MD_CTX_free(context);close(fd);
    if (!good) throw std::runtime_error("candidate identity digest failed");
    constexpr char hex[]="0123456789abcdef";std::string result(64,'0');
    for (unsigned i=0;i<32;++i) { result[i*2]=hex[digest[i]>>4];result[i*2+1]=hex[digest[i]&15]; }
    return result;
}
inline constexpr bool sha256Pin(std::string_view pin) noexcept {
    if (pin.size()!=64) return false;
    for (auto c : pin) if (!((c>='0' && c<='9') || (c>='a' && c<='f'))) return false;
    return true;
}
#ifdef SLEDGE_STOCK_QUALIFIED
// Qualification is a trusted build/package decision, never a runtime/env grant.
// Same-UID peer credentials are user-process trust, not binary attestation or a
// credential/root grant. Daemon independently binds descriptor and mapped inode.
inline void verifyQualified(std::string_view sdk) {
    static_assert(sha256Pin(SLEDGE_STOCK_ELF_SHA256));
    static_assert(sha256Pin(SLEDGE_STOCK_BUILD_ID));
    static_assert((SLEDGE_STOCK_QUALIFICATION_REVISION==1 && SLEDGE_STOCK_FEATURE_MASK==1) ||
                  (SLEDGE_STOCK_QUALIFICATION_REVISION==2 && SLEDGE_STOCK_FEATURE_MASK==3));
    if (sdk!=SLEDGE_STOCK_SDK_API_HASH || startupDigest("/proc/self/exe")!=SLEDGE_STOCK_ELF_SHA256)
        throw std::runtime_error("qualified ELF/SDK mismatch; hooks not installed");
}
#endif
#ifdef SLEDGE_STOCK_CANDIDATE
inline void verifyCandidate(std::string_view sdk) {
    static_assert(sha256Pin(SLEDGE_STOCK_ELF_SHA256));
    static_assert(sha256Pin(SLEDGE_STOCK_MACHINE_ID_SHA256));
    static_assert(sha256Pin(SLEDGE_STOCK_BOOT_MARKER_SHA256));
    constexpr std::string_view marker=SLEDGE_STOCK_BOOT_MARKER_PATH;
    static_assert(!marker.empty() && marker.front()=='/');
    if (sdk!=SLEDGE_STOCK_SDK_API_HASH ||
        startupDigest("/proc/self/exe")!=SLEDGE_STOCK_ELF_SHA256 ||
        startupDigest("/etc/machine-id",true)!=SLEDGE_STOCK_MACHINE_ID_SHA256 ||
        startupDigest(SLEDGE_STOCK_BOOT_MARKER_PATH,true)!=SLEDGE_STOCK_BOOT_MARKER_SHA256)
        throw std::runtime_error("isolated candidate scope/ELF/SDK mismatch; hooks not installed");
}
#endif
}
