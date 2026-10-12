#pragma once
#include "stock_wire.hpp"
#include <poll.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <fcntl.h>
#include <unistd.h>
#include <cerrno>
#include <cstring>
#include <string>
#include <thread>
#include <atomic>

namespace sledge::hyprland {
// Native client / Rust private listener. All endpoint/socket operations run on
// this worker, never callbacks. Passive Hello has zero header deadline; its local
// exchange budget is 10ms. Event transaction deadlines remain bounded/fresh.
class StockWorker {
    DecisionMailbox& mailbox;
    std::atomic<bool> ready{false};
    std::atomic<bool> acknowledged{false};
    std::array<WirePacket,16> notifications{};
    std::atomic<unsigned> writeIndex{0}, readIndex{0};
    std::jthread thread;
    static bool wait(int fd, short events, uint64_t deadline) noexcept {
        const auto now=monotonicNs();
        if (!now || now>=deadline) return false;
        pollfd descriptor{fd,events,0};
        const auto milliseconds=static_cast<int>((deadline-now+999'999)/1'000'000);
        return poll(&descriptor,1,milliseconds)>0 && (descriptor.revents&events) &&
            !(descriptor.revents&(POLLERR|POLLHUP|POLLNVAL));
    }
    static bool exchange(int fd, const WirePacket& request, WirePacket& reply, uint64_t deadline) noexcept {
        if (!wait(fd,POLLOUT,deadline) || send(fd,request.data(),request.size(),MSG_NOSIGNAL)!=512 ||
            !wait(fd,POLLIN,deadline)) return false;
        return recv(fd,reply.data(),reply.size(),MSG_TRUNC)==512 && monotonicNs()<deadline;
    }
    static int connectPrivate(const std::string& path) {
        const auto slash=path.rfind('/');
        if (path.empty() || path.front()!='/' || slash==std::string::npos || slash+1==path.size()) return -1;
        const auto name=path.substr(slash+1);
        if (name=="." || name=="..") return -1;
        const auto directory=open(path.substr(0,slash).c_str(),O_RDONLY|O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC);
        if (directory<0) return -1;
        struct stat parent{}, endpoint{};
        const bool authorized=fstat(directory,&parent)==0 && parent.st_uid==getuid() && !(parent.st_mode&077) &&
            fstatat(directory,name.c_str(),&endpoint,AT_SYMLINK_NOFOLLOW)==0 && S_ISSOCK(endpoint.st_mode) &&
            endpoint.st_uid==getuid() && !(endpoint.st_mode&077);
        const auto anchored="/proc/self/fd/"+std::to_string(directory)+"/"+name;
        sockaddr_un address{};
        if (!authorized || anchored.size()>=sizeof(address.sun_path)) { close(directory); return -1; }
        const auto fd=socket(AF_UNIX,SOCK_SEQPACKET|SOCK_CLOEXEC|SOCK_NONBLOCK,0);
        if (fd<0) { close(directory); return -1; }
        address.sun_family=AF_UNIX;
        std::memcpy(address.sun_path,anchored.c_str(),anchored.size()+1);
        bool connected=connect(fd,reinterpret_cast<const sockaddr*>(&address),sizeof(address))==0;
        if (!connected && errno==EINPROGRESS) {
            connected=wait(fd,POLLOUT,monotonicNs()+10'000'000);
            int error=0; socklen_t size=sizeof(error);
            connected=connected && getsockopt(fd,SOL_SOCKET,SO_ERROR,&error,&size)==0 && !error;
        }
        close(directory);
        ucred peer{}; socklen_t length=sizeof(peer);
        if (!connected || getsockopt(fd,SOL_SOCKET,SO_PEERCRED,&peer,&length)!=0 ||
            length!=sizeof(peer) || peer.uid!=getuid()) { close(fd); return -1; }
        return fd;
    }
    void run(std::stop_token stop, const std::string& path, const WirePacket& hello) {
        const auto fd=connectPrivate(path);
        if (fd<0) return;
        WirePacket ack{};
        if (!exchange(fd,hello,ack,monotonicNs()+10'000'000) || !wire::helloAck(hello,ack)) { close(fd); return; }
        acknowledged.store(true,std::memory_order_release);
        ready.store(true,std::memory_order_release);
        while (!stop.stop_requested()) {
            const auto read=readIndex.load(std::memory_order_relaxed);
            if (read!=writeIndex.load(std::memory_order_acquire)) {
                const auto& packet=notifications[read%notifications.size()];
                const auto deadline=wire::get(packet,40,8);
                if (!wait(fd,POLLOUT,deadline) || send(fd,packet.data(),packet.size(),MSG_NOSIGNAL)!=512) break;
                readIndex.store(read+1,std::memory_order_release);
                continue;
            }
            WirePacket event{}, reply{}; uint64_t deadline=0;
            if (!mailbox.take(event,deadline)) {
                pollfd idle{fd,POLLIN,0};
                // Reject unsolicited Inject/packets, and observe disconnection.
                if (poll(&idle,1,1)!=0) break;
                continue;
            }
            const bool accepted=exchange(fd,event,reply,deadline) && wire::decision(event,reply);
            mailbox.complete(accepted ? reply : WirePacket{});
            if (!accepted) break;
        }
        ready.store(false,std::memory_order_release);
        close(fd);
    }
public:
    StockWorker(DecisionMailbox& decisions, std::string endpoint, WirePacket hello) : mailbox(decisions),
        thread([this,endpoint=std::move(endpoint),hello](std::stop_token stop) {
            try { run(stop,endpoint,hello); } catch (...) { ready.store(false); }
        }) {}
    StockWorker(const StockWorker&) = delete;
    StockWorker& operator=(const StockWorker&) = delete;
    ~StockWorker() { thread.request_stop(); thread.join(); }
    bool notify(const WirePacket& packet) noexcept {
        if (!connected()) return false;
        const auto write=writeIndex.load(std::memory_order_relaxed);
        if (write-readIndex.load(std::memory_order_acquire)==notifications.size()) return false;
        notifications[write%notifications.size()]=packet;
        writeIndex.store(write+1,std::memory_order_release);
        return true;
    }
    bool connected() const noexcept { return ready.load(std::memory_order_acquire); }
    bool handshakeAcknowledged() const noexcept { return acknowledged.load(std::memory_order_acquire); }
};
} // namespace sledge::hyprland
