#pragma once
#include "causal_timing.hpp"
#include "stock_wire.hpp"
#include <cstdio>
#include <cstdlib>
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
public:
    enum class Reason : unsigned {
        ConnectFailed, HelloSendFailed, HelloReceiveTimeout, HelloReceiveMalformed,
        HelloLate, HelloInvalid, Authenticated, RequestedClose, Stopped,
        IdleHangup, IdleUnsolicited, IdleError, NotifyFailed, StaleEvent,
        DecisionFailed, Exception, Count
    };
private:
    std::array<std::atomic<uint64_t>,static_cast<unsigned>(Reason::Count)> reasons{};
    void count(Reason reason) noexcept { reasons[static_cast<unsigned>(reason)].fetch_add(1,std::memory_order_relaxed); }
    CausalTiming* timing=nullptr;
    void observe(CausalTiming::Phase phase,const WirePacket& packet,uint64_t generation,bool ok=true) noexcept {
        if (timing) timing->record(phase,packet,generation,ok);
    }
    bool observedSend(int fd,const WirePacket& packet,uint64_t generation,CausalTiming::Phase start) noexcept {
        observe(start,packet,generation);
        return send(fd,packet.data(),packet.size(),MSG_NOSIGNAL)==512;
    }
    DecisionMailbox& mailbox;
    std::atomic<bool> ready{false};
    std::atomic<bool> acknowledged{false}, closeRequested{false};
    struct Notification { WirePacket packet{}; uint64_t generation=0; };
    std::array<Notification,16> notifications{};
    std::atomic<uint64_t> connectionGeneration{0}, connectionNonce{0};
    std::atomic<unsigned> writeIndex{0}, readIndex{0};
    bool tracePackets=false;
    unsigned traceCount=0;
    void trace(const WirePacket& packet,const char* direction,uint64_t generation) {
        if (!tracePackets || traceCount>=16) return;
        std::array<char,1025> hex{};
        constexpr char digits[]="0123456789abcdef";
        for (unsigned i=0;i<512;++i) { hex[i*2]=digits[packet[i]>>4];hex[i*2+1]=digits[packet[i]&15]; }
        std::fprintf(stderr,"SLEDGE_WORKER_PACKET direction=%s generation=%llu hex=%s\n",direction,
            static_cast<unsigned long long>(generation),hex.data());
        ++traceCount;
    }
    std::atomic<bool> transactionActive{false}, parked{false};
    std::atomic<uint64_t> retirementGeneration{0},closedGeneration{0};
    std::atomic<bool> parkRequested{false};
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
    void runConnection(std::stop_token stop, const std::string& path, const WirePacket& hello, uint64_t generation) {
        const auto fd=connectPrivate(path);
        if (fd<0) { count(Reason::ConnectFailed); return; }
        WirePacket ack{};
        const auto helloDeadline=monotonicNs()+10'000'000;
        if (!wait(fd,POLLOUT,helloDeadline) || send(fd,hello.data(),hello.size(),MSG_NOSIGNAL)!=512) {
            count(Reason::HelloSendFailed);close(fd);return;
        }
        if (!wait(fd,POLLIN,helloDeadline)) {
            count(Reason::HelloReceiveTimeout);close(fd);return;
        }
        if (recv(fd,ack.data(),ack.size(),MSG_TRUNC)!=512) {
            count(Reason::HelloReceiveMalformed);close(fd);return;
        }
        if (monotonicNs()>=helloDeadline) { count(Reason::HelloLate);close(fd);return; }
        if (!wire::helloAck(hello,ack)) { count(Reason::HelloInvalid);close(fd);return; }
        count(Reason::Authenticated);
        acknowledged.store(true,std::memory_order_release);
        connectionNonce.store(wire::get(hello,48,8),std::memory_order_relaxed);
        connectionGeneration.store(generation,std::memory_order_release);
        ready.store(true,std::memory_order_release);
        while (!stop.stop_requested() && !closeRequested.load(std::memory_order_acquire)) {
            if (parkRequested.load(std::memory_order_acquire) &&
                readIndex.load(std::memory_order_acquire)==writeIndex.load(std::memory_order_acquire) && mailbox.idle()) {
                parked.store(true,std::memory_order_release);
                std::this_thread::sleep_for(std::chrono::milliseconds(1));
                continue;
            }
            const auto read=readIndex.load(std::memory_order_relaxed);
            if (read!=writeIndex.load(std::memory_order_acquire)) {
                const auto notification=notifications[read%notifications.size()];
                const auto& packet=notification.packet;
                if (notification.generation!=generation || wire::get(packet,48,8)!=wire::get(hello,48,8)) {
                    readIndex.store(read+1,std::memory_order_release);
                    continue;
                }
                const auto deadline=wire::get(packet,40,8);
                if (!wait(fd,POLLOUT,deadline) ||
                    !(wire::get(packet,6,2)==5 ? observedSend(fd,packet,generation,CausalTiming::Phase::OutcomeSendStart) :
                      send(fd,packet.data(),packet.size(),MSG_NOSIGNAL)==512)) {
                    if (wire::get(packet,6,2)==5) observe(CausalTiming::Phase::OutcomeSend,packet,generation,false);
                    count(Reason::NotifyFailed);break;
                }
                if (wire::get(packet,6,2)==5) observe(CausalTiming::Phase::OutcomeSend,packet,generation);
                trace(packet,"notify",generation);
                readIndex.store(read+1,std::memory_order_release);
                continue;
            }
            WirePacket event{}, reply{}; uint64_t deadline=0;
            if (!mailbox.take(event,deadline)) {
                pollfd idle{fd,POLLIN,0};
                // Reject unsolicited Inject/packets, and observe disconnection.
                const auto result=poll(&idle,1,1);
                if (result<0) {
                    if (errno==EINTR) continue;
                    count(Reason::IdleError);break;
                }
                if (result>0) {
                    if (idle.revents&(POLLHUP|POLLERR|POLLNVAL)) count(Reason::IdleHangup);
                    else count(Reason::IdleUnsolicited);
                    break;
                }
                continue;
            }
            transactionActive.store(true,std::memory_order_release);
            trace(event,"event",generation);
            const bool sent=wire::get(event,48,8)==wire::get(hello,48,8) &&
                wait(fd,POLLOUT,deadline) && observedSend(fd,event,generation,CausalTiming::Phase::EventSendStart);
            observe(CausalTiming::Phase::EventSend,event,generation,sent);
            const bool received=sent && wait(fd,POLLIN,deadline) && recv(fd,reply.data(),reply.size(),MSG_TRUNC)==512;
            const auto receiveAt=monotonicNs();
            const bool accepted=received && receiveAt<deadline && wire::decision(event,reply);
            if (timing) timing->record(CausalTiming::Phase::DecisionReceive,event,generation,received,receiveAt);
            trace(reply,"reply",generation);
            observe(CausalTiming::Phase::MailboxPublish,event,generation,accepted);
            mailbox.complete(accepted ? reply : WirePacket{});
            transactionActive.store(false,std::memory_order_release);
            if (!accepted) {
                count(wire::get(event,48,8)!=wire::get(hello,48,8) ? Reason::StaleEvent : Reason::DecisionFailed);
                break;
            }
        }
        if (stop.stop_requested()) count(Reason::Stopped);
        ready.store(false,std::memory_order_release);
        close(fd);
        closedGeneration.store(generation,std::memory_order_release);
    }
    void run(std::stop_token stop,const std::string& path,WirePacket hello) {
        const auto seed=wire::get(hello,48,8);
        for (uint64_t generation=1; !stop.stop_requested() && generation; ++generation) {
            if (parkRequested.load(std::memory_order_acquire)) {
                // runConnection has returned: the old socket is closed and no
                // transaction can still emit. Only an owner-verified drain
                // barrier may settle receipts scoped to that generation.
                const auto retired=retirementGeneration.load(std::memory_order_acquire);
                if (retired && mailbox.idle() && !transactionActive.load(std::memory_order_acquire)) {
                    auto read=readIndex.load(std::memory_order_relaxed);
                    const auto write=writeIndex.load(std::memory_order_acquire);
                    while (read!=write && notifications[read%notifications.size()].generation==retired) ++read;
                    readIndex.store(read,std::memory_order_release);
                }
                if (mailbox.idle()) parked.store(true,std::memory_order_release);
                std::this_thread::sleep_for(std::chrono::milliseconds(1));
                --generation;continue;
            }
            const auto nonce=seed+generation-1;
            if (!nonce) break; // never reuse after nonce wrap
            wire::put(hello,48,8,nonce);
            closeRequested.store(false,std::memory_order_release);
            runConnection(stop,path,hello,generation);
            for (unsigned i=0;i<25 && !stop.stop_requested();++i)
                std::this_thread::sleep_for(std::chrono::milliseconds(10));
        }
    }
public:
    StockWorker(DecisionMailbox& decisions, std::string endpoint, WirePacket hello,CausalTiming* observations=nullptr) : timing(observations),mailbox(decisions),
        thread([this,endpoint=std::move(endpoint),hello](std::stop_token stop) {
#ifndef SLEDGE_STOCK_PRODUCTION
            if (const auto* enabled=std::getenv("SLEDGE_STOCK_RESEARCH_WIRE_TRACE")) tracePackets=std::strcmp(enabled,"1")==0;
#endif
            try { run(stop,endpoint,hello); } catch (...) { count(Reason::Exception);ready.store(false); }
        }) {}
    StockWorker(const StockWorker&) = delete;
    StockWorker& operator=(const StockWorker&) = delete;
    ~StockWorker() { thread.request_stop(); thread.join(); }
    void invalidate() noexcept {
        count(Reason::RequestedClose);
        ready.store(false,std::memory_order_release);
        closeRequested.store(true,std::memory_order_release);
    }
    bool notify(const WirePacket& packet) noexcept {
        const auto generation=connectionGeneration.load(std::memory_order_acquire);
        const auto rejected=[&] {
            if (wire::get(packet,6,2)==5) observe(CausalTiming::Phase::OutcomeEnqueue,packet,generation,false);
            return false;
        };
        if (!connected() || parkRequested.load(std::memory_order_acquire)) return rejected();
        const auto write=writeIndex.load(std::memory_order_relaxed);
        if (write-readIndex.load(std::memory_order_acquire)==notifications.size()) return rejected();
        if (wire::get(packet,48,8)!=connectionNonce.load(std::memory_order_acquire)) return rejected();
        if (wire::get(packet,6,2)==5) observe(CausalTiming::Phase::OutcomeEnqueue,packet,generation);
        notifications[write%notifications.size()]={packet,generation};
        writeIndex.store(write+1,std::memory_order_release);
        return true;
    }
    void requestPark() noexcept { parkRequested.store(true,std::memory_order_release); }
    // Caller must have disabled admission and proved physical/output ledgers
    // released, with no uncertainty or mailbox obligation. This retires receipt
    // delivery to a closed generation, NEVER acknowledges remote receipt.
    bool retireGenerationForDrain(uint64_t generation,bool ownerVerifiedBarrier) noexcept {
        if (!ownerVerifiedBarrier || !generation || generation!=this->generation() ||
            !parkRequested.load(std::memory_order_acquire) || !mailbox.idle()) return false;
        if (retirementGeneration.load(std::memory_order_acquire)==generation) return true;
        retirementGeneration.store(generation,std::memory_order_release);
        invalidate(); // close before queue retirement, also prevents reconnect
        return true;
    }
    bool idleParked() const noexcept {
        const auto retired=retirementGeneration.load(std::memory_order_acquire);
        return (!retired || closedGeneration.load(std::memory_order_acquire)>=retired) &&
            parked.load(std::memory_order_acquire) && mailbox.idle() &&
            !transactionActive.load(std::memory_order_acquire) &&
            readIndex.load(std::memory_order_acquire)==writeIndex.load(std::memory_order_acquire);
    }
    uint64_t reasonCount(Reason reason) const noexcept {
        return reasons[static_cast<unsigned>(reason)].load(std::memory_order_relaxed);
    }
    uint64_t generation() const noexcept { return connectionGeneration.load(std::memory_order_acquire); }
    uint64_t nonce() const noexcept { return connectionNonce.load(std::memory_order_acquire); }
    bool connected() const noexcept { return ready.load(std::memory_order_acquire); }
    bool handshakeAcknowledged() const noexcept { return acknowledged.load(std::memory_order_acquire); }
};
} // namespace sledge::hyprland
