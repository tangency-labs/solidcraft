// SPDX-License-Identifier: Apache-2.0
// Fresh-process containment: never allocate SMLib objects in a forked image of
// the multithreaded Rust host. posix_spawn executes a dedicated worker first.
#ifndef SC_ROLLOVER_WORKER_H
#define SC_ROLLOVER_WORKER_H
#include "sc_shim.h"
#include <array>
#include <cerrno>
#include <cstddef>
#include <cstdio>
#include <cstring>
#if defined(__linux__) || defined(__APPLE__)
#include <fcntl.h>
#include <poll.h>
#include <signal.h>
#include <spawn.h>
#include <sys/wait.h>
#include <unistd.h>
extern char** environ;
#endif
// Containment failures, one status per stage, distinct from SC_ERR_EXCEPTION (which
// stays the worker's own report of a C++ exception caught at the shim boundary).
// Mirrored by solidcore-ffi's status names; each also writes its stage and errno to
// stderr.
#define SC_ERR_WORKER_PIPE (-1120)    // pipe2 / request write failed
#define SC_ERR_WORKER_SPAWN (-1121)   // posix_spawn (or its file actions) failed
#define SC_ERR_WORKER_TIMEOUT (-1122) // no reply within the poll timeout; worker killed
#define SC_ERR_WORKER_IO (-1123)      // short or failed read, or a malformed reply header
#define SC_ERR_WORKER_EXIT (-1124)    // worker exited with a nonzero code
#define SC_ERR_WORKER_SIGNAL (-1125)  // worker killed by a signal (native crash or abort)
#define SC_ERR_WORKER_WAIT (-1126)    // waitpid did not reap the worker
namespace sc_rollover {
constexpr size_t INPUT_LIMIT=70, OUTPUT_LIMIT=4096;
struct Request { int64_t operation,count;
#ifdef SC_ROLLOVER_FAULT_TEST
    int64_t fail_new,fail_calloc,signal;
#endif
};
struct Header { int64_t status,length; };
#ifdef SC_ROLLOVER_FAULT_TEST
inline int64_t fail_new=-1,fail_calloc=-1,test_signal=-1;
#endif
#if defined(__linux__) || defined(__APPLE__)
inline bool cloexec_pipe(int fds[2]) {
#if defined(__linux__)
    return ::pipe2(fds,O_CLOEXEC)==0;
#else
    if(::pipe(fds)!=0) return false;
    if(::fcntl(fds[0],F_SETFD,FD_CLOEXEC)==0 &&
       ::fcntl(fds[1],F_SETFD,FD_CLOEXEC)==0) return true;
    ::close(fds[0]);::close(fds[1]);
    return false;
#endif
}
inline int spawn_worker(pid_t* child,const char* path,
                        const posix_spawn_file_actions_t* actions,char* const argv[]) {
#if defined(__APPLE__)
    // Darwin has no atomic pipe2(O_CLOEXEC). Close every descriptor except the
    // explicit dup2 actions, including pipes another thread has just created
    // but has not yet marked CLOEXEC. No worker can inherit another's pipes.
    posix_spawnattr_t attributes;
    int error=::posix_spawnattr_init(&attributes);
    if(error!=0) return error;
    error=::posix_spawnattr_setflags(&attributes,POSIX_SPAWN_CLOEXEC_DEFAULT);
    if(error==0) error=::posix_spawn(child,path,actions,&attributes,argv,environ);
    ::posix_spawnattr_destroy(&attributes);
    return error;
#else
    return ::posix_spawn(child,path,actions,nullptr,argv,environ);
#endif
}
inline bool write_all(int fd, const void* data, size_t n) {
    const char* p=static_cast<const char*>(data);
    while(n) { ssize_t sent=::write(fd,p,n); if(sent<0 && errno==EINTR) continue;
        if(sent<=0) return false; p+=sent; n-=static_cast<size_t>(sent); }
    return true;
}
enum class ReadResult { Ok, Timeout, Short, Error };
// Reads exactly n bytes, waiting at most 30 s for each chunk.
inline ReadResult read_exact(int fd, void* data, size_t n) {
    char* p=static_cast<char*>(data);
    while(n) {
        pollfd event{fd,POLLIN,0};
        int ready; do { ready=::poll(&event,1,30000); } while(ready<0 && errno==EINTR);
        if(ready==0) return ReadResult::Timeout;
        if(ready<0) return ReadResult::Error;
        ssize_t got=::read(fd,p,n); if(got<0 && errno==EINTR) continue;
        if(got==0) return ReadResult::Short;
        if(got<0) return ReadResult::Error;
        p+=got; n-=static_cast<size_t>(got);
    }
    return ReadResult::Ok;
}
inline bool read_all(int fd, void* data, size_t n) { return read_exact(fd,data,n)==ReadResult::Ok; }
// One stderr line per containment failure: the stage, errno and the worker's pid.
inline int32_t stage_failure(int32_t status, const char* stage, int error, pid_t child) {
    std::fprintf(stderr,"sc_rollover: worker %s failed (status %d, errno %d: %s, pid %ld)\n",
                 stage,static_cast<int>(status),error,error?std::strerror(error):"none",
                 static_cast<long>(child));
    return status;
}
#endif
// One bounded request is written into a private pipe before spawn: no parent
// write can race a dying child and deliver SIGPIPE. Pipes are CLOEXEC; Darwin
// also closes all non-explicit descriptors at spawn to cover pipe/fcntl races.
inline int32_t isolated(int64_t operation,const double* input,int64_t count,
                        double* out,int64_t cap,int64_t* len) {
#if (defined(__linux__) || defined(__APPLE__)) && defined(SC_ROLLOVER_WORKER_PATH)
    if(count<0 || count>static_cast<int64_t>(INPUT_LIMIT)) return SC_ERR_ARG;
    int in[2],result[2];
    if(!cloexec_pipe(in)) return stage_failure(SC_ERR_WORKER_PIPE,"pipe2",errno,-1);
    if(!cloexec_pipe(result)) {
        int error=errno;
        ::close(in[0]);::close(in[1]);
        return stage_failure(SC_ERR_WORKER_PIPE,"pipe2",error,-1);
    }
    Request request{operation,count
#ifdef SC_ROLLOVER_FAULT_TEST
        ,fail_new,fail_calloc,test_signal
#endif
    };
    bool okay=write_all(in[1],&request,sizeof(request)) &&
        write_all(in[1],input,static_cast<size_t>(count)*sizeof(double));
    int write_error=okay?0:errno;
    ::close(in[1]);
    if(!okay) {
        ::close(in[0]);::close(result[0]);::close(result[1]);
        return stage_failure(SC_ERR_WORKER_PIPE,"request write",write_error,-1);
    }
    posix_spawn_file_actions_t actions;
    int error=::posix_spawn_file_actions_init(&actions);
    if(error!=0) {
        ::close(in[0]);::close(result[0]);::close(result[1]);
        return stage_failure(SC_ERR_WORKER_SPAWN,"posix_spawn_file_actions_init",error,-1);
    }
    error=::posix_spawn_file_actions_adddup2(&actions,in[0],STDIN_FILENO);
    if(error==0) error=::posix_spawn_file_actions_adddup2(&actions,result[1],STDOUT_FILENO);
    const char* path=SC_ROLLOVER_WORKER_PATH;
    char* argv[]={const_cast<char*>(path),const_cast<char*>("--rollover-worker"),nullptr};
    pid_t child=-1;
    if(error==0) error=spawn_worker(&child,path,&actions,argv);
    ::posix_spawn_file_actions_destroy(&actions);
    ::close(in[0]);::close(result[1]);
    if(error!=0) {::close(result[0]);return stage_failure(SC_ERR_WORKER_SPAWN,"posix_spawn",error,-1);}
    Header header{SC_ERR_EXCEPTION,0};
    std::array<double,OUTPUT_LIMIT> buffer{};
    ReadResult read=read_exact(result[0],&header,sizeof(header));
    int read_error=read==ReadResult::Error?errno:0;
    bool malformed=read==ReadResult::Ok &&
        (header.length<0 || header.length>static_cast<int64_t>(OUTPUT_LIMIT));
    if(read==ReadResult::Ok && !malformed && header.status==SC_OK) {
        read=read_exact(result[0],buffer.data(),static_cast<size_t>(header.length)*sizeof(double));
        if(read==ReadResult::Error) read_error=errno;
    }
    ::close(result[0]);
    bool replied=read==ReadResult::Ok && !malformed;
    if(!replied) ::kill(child,SIGKILL); // only our own failed/stalled worker
    int status=0;pid_t reaped;
    do {reaped=::waitpid(child,&status,0);} while(reaped<0 && errno==EINTR);
    int wait_error=reaped<0?errno:0;
    // A stalled worker is the timeout, whatever our SIGKILL made of its exit status.
    if(read==ReadResult::Timeout) return stage_failure(SC_ERR_WORKER_TIMEOUT,"reply (30 s timeout)",0,child);
    if(reaped!=child) return stage_failure(SC_ERR_WORKER_WAIT,"waitpid",wait_error,child);
    // A worker that died before replying in full reports how it died. A SIGKILL after a
    // failed read is ours (the read stage is reported below).
    if(WIFSIGNALED(status) && !(!replied && WTERMSIG(status)==SIGKILL)) {
        std::fprintf(stderr,"sc_rollover: worker pid %ld killed by signal %d\n",
                     static_cast<long>(child),WTERMSIG(status));
        return SC_ERR_WORKER_SIGNAL;
    }
    if(WIFEXITED(status) && WEXITSTATUS(status)!=0) {
        std::fprintf(stderr,"sc_rollover: worker pid %ld exited with code %d\n",
                     static_cast<long>(child),WEXITSTATUS(status));
        return SC_ERR_WORKER_EXIT;
    }
    if(!replied) return stage_failure(SC_ERR_WORKER_IO,malformed?"reply header":"reply read",read_error,child);
    if(!WIFEXITED(status)) return stage_failure(SC_ERR_WORKER_WAIT,"waitpid status",0,child);
    if(header.status!=SC_OK) return static_cast<int32_t>(header.status);
    return sc_copy_out(buffer.data(),header.length,out,cap,len);
#else
    (void)operation;(void)input;(void)count;(void)out;(void)cap;(void)len;
    return SC_ERR_EXCEPTION;
#endif
}
}
#endif
